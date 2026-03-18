from fastapi import FastAPI, HTTPException, WebSocket, WebSocketDisconnect
from fastapi.middleware.cors import CORSMiddleware
from fastapi.responses import HTMLResponse
from pydantic import BaseModel
import json
from typing import TYPE_CHECKING, cast, Any
if TYPE_CHECKING:
    from google.genai.types import FunctionCall
import asyncio
from contextlib import asynccontextmanager
from . import database
from .models import TaskCreate, UserCreate, UserLogin
import os
from . import ai_tools
from . import commute_scheduler
import bcrypt

async def enrich_commute_ticket(task_id: str, origin: str, destination: str) -> None:
    """
    Background task to fetch directions and update a ticket's payload.
    
    Args:
        task_id: The UUID of the task to enrich
        origin: Trip starting point
        destination: Trip destination
    """
    try:
        from . import ai_tools
        raw_resp: dict[str, object] = await ai_tools.call_directions_service(origin, destination)
        raw_routes = raw_resp.get("routes", [])
        if not isinstance(raw_routes, list):
            raw_routes = []
            
        parsed = ai_tools.parse_transit_directions(raw_routes)
        if parsed:
            # Fetch current task to avoid overwriting other fields
            task_record = await database.get_task(task_id)
            if not task_record:
                return
            
            p_load: dict[str, Any] = json.loads(task_record["payload"])
            directions: dict[str, Any] = p_load.get("directions", {})
            directions.update({
                "steps": parsed[0].get("steps", []),
                "total_duration": parsed[0].get("total_duration", "Unknown"),
                "departure_time": parsed[0].get("departure_time", ""),
                "arrival_time": parsed[0].get("arrival_time", ""),
                "error": None
            })
            p_load["directions"] = directions
            await database.update_task_payload(task_id, json.dumps(p_load))
    except Exception as e:
        print(f"Background enrichment failed for ticket {task_id}: {e}")
        try:
            task_record = await database.get_task(task_id)
            if task_record:
                p_load: dict[str, Any] = json.loads(task_record["payload"])
                directions: dict[str, Any] = p_load.get("directions", {})
                directions["error"] = f"Information unavailable: {e}"
                p_load["directions"] = directions
                await database.update_task_payload(task_id, json.dumps(p_load))
        except Exception:
            pass

def verify_password(plain_password: str, hashed_password: str) -> bool:
    """
    Verify a plain text password against a bcrypt hash.
    
    Args:
        plain_password: The password to check
        hashed_password: The stored hash
        
    Returns:
        True if the password matches, False otherwise
    """
    try:
        return bcrypt.checkpw(
            plain_password[:72].encode('utf-8'),
            hashed_password.encode('utf-8')
        )
    except Exception:
        return False

def get_password_hash(password: str) -> str:
    """
    Generate a bcrypt hash for a password.
    
    Args:
        password: The plain text password
        
    Returns:
        The hex-encoded hash string
    """
    return bcrypt.hashpw(
        password[:72].encode('utf-8'),
        bcrypt.gensalt()
    ).decode('utf-8')

class ConnectionManager:
    def __init__(self):
        self.active_connections: dict[int, list[WebSocket]] = {}

    async def connect(self, websocket: WebSocket, userid: int):
        await websocket.accept()
        if userid not in self.active_connections:
            self.active_connections[userid] = []
        self.active_connections[userid].append(websocket)

    def disconnect(self, websocket: WebSocket, userid: int):
        if userid in self.active_connections:
            try:
                self.active_connections[userid].remove(websocket)
                if not self.active_connections[userid]:
                    del self.active_connections[userid]
            except ValueError:
                pass

    async def broadcast_state_update(self, userid: int):
        if userid in self.active_connections:
            try:
                state_hash = await database.calculate_state_hash(userid)
                message = json.dumps({"type": "STATE_UPDATED", "new_hash": state_hash})
                for connection in self.active_connections[userid]:
                    await connection.send_text(message)
            except Exception as e:
                print(f"Error broadcasting state update: {e}")

manager = ConnectionManager()

@asynccontextmanager
async def lifespan(app: FastAPI):
    try:
        await database.connect_db()
        print("Database connected successfully!")
    except Exception as e:
        print(f"Database connection failed (mock mode active): {e}")
    # Start the commute scheduler with a broadcast callback
    scheduler_task = asyncio.create_task(commute_scheduler.run_scheduler(manager.broadcast_state_update))
    yield
    scheduler_task.cancel()
    try:
        await scheduler_task
    except asyncio.CancelledError:
        pass
    try:
        await database.close_db()
    except Exception:
        pass

app = FastAPI(lifespan=lifespan)

app.add_middleware(
    CORSMiddleware,
    allow_origins=["*"],
    allow_credentials=True,
    allow_methods=["*"],
    allow_headers=["*"],
)


@app.get("/", response_class=HTMLResponse)
async def read_root():
    index_path = os.path.join("static", "index.html")
    if os.path.exists(index_path):
        with open(index_path, "r") as f:
            return HTMLResponse(content=f.read())
    return HTMLResponse(content="<h1>Welcome</h1>")

@app.websocket("/ws/sync/{userid}")
async def websocket_sync_endpoint(websocket: WebSocket, userid: int):
    await manager.connect(websocket, userid)
    try:
        while True:
            data_str = await websocket.receive_text()
            try:
                data = json.loads(data_str)
                # Client Hello Handshake
                if data.get("type") == "HELLO":
                    client_hash = data.get("client_hash")
                    server_hash = await database.calculate_state_hash(userid)
                    if client_hash == server_hash:
                        await websocket.send_text(json.dumps({"type": "ACK", "status": "IN_SYNC"}))
                    else:
                        await websocket.send_text(json.dumps({
                            "type": "OUT_OF_SYNC", 
                            "server_hash": server_hash
                        }))
                
                elif data.get("type") == "SYNC_ACTIONS":
                    # Parse actions and reply
                    actions = data.get("actions", [])
                    ack_ids = []
                    for action in actions:
                        action_type = action.get("type")
                        action_id = action.get("action_id")
                        entity_id = action.get("entity_id")
                        entity_type = action.get("entity_type", "TASK")
                        payload = action.get("payload")
                        
                        try:
                            # 1. Log Event to event store
                            await database.save_sync_event(
                                userid=userid,
                                action_id=action_id,
                                action_type=action_type,
                                entity_id=entity_id,
                                entity_type=entity_type,
                                payload=payload or {}
                            )

                            # 2. Apply Event to Task table (Materialized View style)
                            if action_type == "CREATE":
                                await database.create_task(userid=userid, type=entity_type, payload=json.dumps(payload or {}), status=action.get("status", "idle"))
                            elif action_type == "UPDATE":
                                p_str = json.dumps(payload) if payload is not None else None
                                await database.update_task(task_id=entity_id, payload=p_str, status=action.get("status"))
                            elif action_type == "DELETE":
                                await database.delete_task(task_id=entity_id)

                            ack_ids.append(action_id)
                        except Exception as e:
                            print(f"Failed to process sync action {action_id}: {e}")

                    new_hash = await database.calculate_state_hash(userid)
                    await websocket.send_text(json.dumps({
                        "type": "SYNC_ACK",
                        "ack_action_ids": ack_ids,
                        "server_hash": new_hash
                    }))
                    
                    # Also notify other connections of the update
                    if actions:
                        await manager.broadcast_state_update(userid)
                        
            except json.JSONDecodeError:
                pass
            except Exception:
                pass
    except WebSocketDisconnect:
        manager.disconnect(websocket, userid)

@app.get("/api/sync/events/{userid}")
async def fetch_sync_events(userid: int, after_id: int = 0):
    try:
        events = await database.get_sync_events(userid, after_id)
        current_hash = await database.calculate_state_hash(userid)
        return {"events": events, "server_hash": current_hash}
    except Exception as e:
        raise HTTPException(status_code=500, detail=str(e))

@app.get("/api/tasks")
async def get_tasks(userid: int):
    try:
        tasks = await database.fetch_all_tasks(userid)
        for t in tasks:
            if isinstance(t.get('payload'), str):
                try:
                    payload_str = t['payload']
                    if isinstance(payload_str, str):
                        t['payload'] = json.loads(payload_str)
                except Exception:
                    pass
        return tasks
    except Exception as e:
        raise HTTPException(status_code=500, detail=str(e))

@app.post("/api/auth/register")
async def register_user(user: UserCreate):
    try:
        if user.first_name is None:
            raise HTTPException(status_code=400, detail="First name is required")
        existing = await database.get_user_by_name(user.first_name)
        if existing:
            raise HTTPException(status_code=400, detail="User already exists")
        
        hashed = get_password_hash(user.password or "")
        new_user = await database.create_user(user.first_name, user.last_name or "", hashed)
        if new_user:
            return {"id": new_user["id"], "first_name": new_user["first_name"]}
        raise HTTPException(status_code=500, detail="Could not create user")
    except Exception as e:
        raise HTTPException(status_code=500, detail=str(e))

@app.post("/api/auth/login")
async def login_user(user: UserLogin):
    try:
        db_user = await database.get_user_by_name(user.first_name)
        if not db_user:
            raise HTTPException(status_code=400, detail="Invalid username or password")
            
        if not verify_password(user.password, db_user["password"]):
            raise HTTPException(status_code=400, detail="Invalid username or password")
            
        return {"id": db_user["id"], "first_name": db_user["first_name"]}
    except Exception as e:
        raise HTTPException(status_code=500, detail=str(e))

@app.post("/api/tasks")
async def create_task(task: TaskCreate):
    try:
        import uuid
        action_id = str(uuid.uuid4())
        # Pydantic enum `task.type.value` gets the string 'HABIT', 'TASK', etc.
        payload_str = json.dumps(task.payload) if task.payload else "{}"
        
        if task.userid is None:
            raise HTTPException(status_code=400, detail="User ID is required")
            
        # 1. Log Event
        await database.save_sync_event(
            userid=task.userid,
            action_id=action_id,
            action_type="CREATE",
            entity_id=str(uuid.uuid4()), 
            entity_type=task.type.value,
            payload=task.payload or {}
        )
        
        # 2. Materialize
        row = await database.create_task(task.userid, task.type.value, payload_str, status=task.status.value)
        if row:
            res = dict(row)
            if isinstance(res.get('payload'), str):
                try:
                    res['payload'] = json.loads(res['payload'])
                except Exception:
                    pass
            if task.userid:
                await manager.broadcast_state_update(task.userid)
            return res
        raise HTTPException(status_code=500, detail="Failed to create task")
    except Exception as e:
        raise HTTPException(status_code=500, detail=str(e))

@app.patch("/api/tasks/{task_id}/status")
async def update_task_status(task_id: str, status: str, userid: int):
    try:
        row = await database.update_task_status(task_id, status)
        if row:
            await manager.broadcast_state_update(userid)
            return dict(row)
        raise HTTPException(status_code=404, detail="Task not found")
    except Exception as e:
        raise HTTPException(status_code=500, detail=str(e))

class ChatRequest(BaseModel):
    message: str
    userid: int


@app.get("/api/commutes/{userid}")
async def get_user_commutes(userid: int) -> list[dict[str, object]]:
    """List all registered commutes for a user."""
    if database.pool is None:
        raise HTTPException(status_code=500, detail="Database not connected")
    async with database.pool.acquire() as conn:
        rows = await conn.fetch(
            "SELECT * FROM public.task WHERE userid = $1 AND type = 'COMMUTE' ORDER BY created_at ASC",
            userid,
        )
    commutes: list[dict[str, object]] = []
    for r in rows:
        d: dict[str, object] = dict(r)
        payload_raw = d.get("payload")
        if isinstance(payload_raw, str):
            try:
                d["payload"] = json.loads(payload_raw)
            except Exception:
                pass
        commutes.append(d)
    return commutes


@app.get("/api/live-trips/{userid}")
async def get_live_trips(userid: int) -> dict[str, list[dict[str, object]]]:
    """List active live/urgent direction trips for a user."""
    return {"trips": commute_scheduler.get_active_live_trips(userid)}


@app.post("/api/chat")
async def chat_with_gemini(req: ChatRequest) -> dict[str, object]:
    """
    Main chat endpoint that processes messages with Gemini and executes tool calls.
    
    Args:
        req: The chat request containing message and user context
        
    Returns:
        JSON response with the action taken or the assistant's message
    """
    if not ai_tools.client:
        return {"action": "message", "response": "Warning: GEMINI_API_KEY is not set in `.env`."}

    # 1. Fetch current context for the user
    current_tickets = await database.fetch_all_tasks(req.userid)
    
    # Custom encoder for UUID and datetime
    def custom_encoder(obj: object) -> str:
        from uuid import UUID
        from datetime import datetime
        if isinstance(obj, UUID):
            return str(obj)
        if isinstance(obj, datetime):
            return obj.isoformat()
        raise TypeError(f"Type {type(obj)} not serializable")

    context_str = json.dumps(current_tickets, indent=2, default=custom_encoder)

    try:
        # Prompt gemini with the system instructions and tools
        response = ai_tools.client.models.generate_content(
            model='gemini-flash-latest',
            contents=req.message,
            config={
                "tools": ai_tools.chat_tools,
                "system_instruction": f"""You are a ticket management assistant for HStack.
    You manage a 'stack' of tickets for the user. 

    CRITICAL: TEMPORAL EXTRACTION & NORMALIZATION
    1. EXTRACT scheduled_time: Mandate extraction of any temporal data (e.g. 15:00).
    2. NORMALIZE scheduled_time: Always convert to HH:MM (24-hour) e.g. "08:00". No conversational text.
    3. SCHEDULING DSL for HABITs:
       - If a HABIT is repetitive, use the `recurrence` parameter with this DSL:
         - Standard: `EVERY DAY`, `WEEKDAYS`, `WEEKENDS`.
         - Days: `MON, WED, FRI` (use 3-letter uppercase).
         - Monthly: `9TH OF MONTH`, `9TH, 10TH OF MONTH`.
         - Ordinal: `1ST MON OF MONTH`, `LAST FRI OF MONTH`.
       - Example: "Add habit to gym on Mon, Wed and Fri at 6pm" -> `scheduled_time: "18:00", recurrence: "MON, WED, FRI"`
       - Example: "I need to pay the landlord on the 1st and 15th of every month at 9am" -> `scheduled_time: "09:00", recurrence: "1ST, 15TH OF MONTH"`
    
    The `scheduled_time` field is what triggers the visual "Scope" side-bars in the UI.

    CURRENT TICKET STACK (JSON):
    {context_str}

    TICKET CATEGORIES:
    - HABIT: Routines (e.g., 'morning coffee').
    - TASK: One-off actions (e.g., 'buy groceries').
    - EVENT: Time-specific appointments (e.g., 'dentist').

    COMMUTE MANAGEMENT:
    - Recurring trips -> `add_commute`.
    - Live/Urgent trips -> `start_live_directions`.
    
    ACTION RULES:
    1. BREAK DOWN complex requests into multiple tool calls.
    2. ALWAYS use the provided tools for state changes.
    3. NO EMOJIS in ticket titles or descriptions. Keep them clean and professional.
    4. Respond with a brief, sexy confirmation of actions taken.""",
            }
        )
        
        # Extract function calls from the response
        func_calls: list[Any] = []
        try:
            if response.candidates:
                parts = getattr(response.candidates[0].content, 'parts', [])
                for part in parts:
                    fc = getattr(part, 'function_call', None)
                    if fc:
                        func_calls.append(fc)
        except Exception:
            # Fallback for different SDK versions or properties
            raw_calls = getattr(response, 'function_calls', [])
            if isinstance(raw_calls, list):
                func_calls = raw_calls

        actions_taken: list[str] = []
        if func_calls:
            for call_obj in func_calls:
                # Use cast if we are sure it's a FunctionCall or has these attributes
                call_name: str = str(getattr(call_obj, "name", ""))
                call_args: dict[str, Any] = cast(dict[str, Any], getattr(call_obj, "args", {}))
                
                if call_name == "create_ticket":
                    ticket_type = str(call_args.get("type", "TASK"))
                    p_load: dict[str, object] = {"completed": False}
                    for key, val in call_args.items():
                        if key != "type":
                            p_load[key] = val
                    _ = await database.create_task(req.userid, ticket_type, json.dumps(p_load), status="idle")
                    actions_taken.append("create")

                elif call_name == "delete_ticket":
                    record_id = str(call_args.get("task_id", ""))
                    try:
                        _ = await database.delete_task(record_id)
                        actions_taken.append("delete")
                    except Exception: pass

                elif call_name == "delete_all_tickets":
                    await database.delete_all_tasks(req.userid)
                    commute_scheduler.clear_user(req.userid)
                    actions_taken.append("clear")

                elif call_name == "edit_ticket":
                    tid = str(call_args.get("task_id", ""))
                    new_type = cast(str | None, call_args.get("type"))
                    new_title = cast(str | None, call_args.get("title"))
                    new_time = cast(str | None, call_args.get("scheduled_time"))
                    new_dur = cast(int | None, call_args.get("duration_minutes"))
                    
                    existing_task = await database.get_task(tid)
                    curr_payload: dict[str, object] = {}
                    if existing_task:
                        p_raw = existing_task.get("payload")
                        if isinstance(p_raw, str):
                            try:
                                curr_payload = cast(dict[str, object], json.loads(p_raw))
                            except Exception: pass
                            
                    if new_title: curr_payload["title"] = new_title
                    if new_time: curr_payload["scheduled_time"] = new_time
                    if new_dur: curr_payload["duration_minutes"] = new_dur
                    _ = await database.update_task(tid, new_type, json.dumps(curr_payload))
                    _ = actions_taken.append("edit")

                elif call_name == "add_commute":
                    label = str(call_args.get("label", "commute"))
                    origin = str(call_args.get("origin", ""))
                    destination = str(call_args.get("destination", ""))
                    deadline = str(call_args.get("deadline", "09:00"))
                    days = str(call_args.get("days", "monday,tuesday,wednesday,thursday,friday"))
                    
                    commute_data: dict[str, object] = {
                        "title": f"{label.replace('_', ' ').title()}: {origin[:30]}… → {destination[:30]}… @ {deadline}",
                        "label": label, "origin": origin, "destination": destination, 
                        "deadline": deadline, "days": days, "completed": False,
                    }
                    _ = await database.create_task(req.userid, "COMMUTE", json.dumps(commute_data), status="idle")
                    _ = actions_taken.append("add_commute")

                elif call_name == "remove_commute":
                    record_id = str(call_args.get("task_id", ""))
                    try:
                        _ = await database.delete_task(record_id)
                        _ = actions_taken.append("remove_commute")
                    except Exception: pass

                elif call_name == "get_directions":
                    origin = str(call_args.get("origin", ""))
                    destination = str(call_args.get("destination", ""))
                    # Create the ticket IMMEDIATELY with a skeleton payload
                    d_load: dict[str, object] = {
                        "title": f"Directions: {origin[:30]}… → {destination[:30]}…",
                        "origin": origin, "destination": destination,
                        "directions": {"steps": [], "total_duration": "Enriching...", "error": None}
                    }
                    created = await database.create_task(req.userid, "COMMUTE", json.dumps(d_load), status="in_focus")
                    if created:
                        # Schedule background enrichment
                        _ = asyncio.create_task(enrich_commute_ticket(str(cast(object, created["id"])), origin, destination))
                    _ = actions_taken.append("get_directions")

                elif call_name == "start_live_directions":
                    origin = str(cast(object, call_args.get("origin", "")))
                    destination = str(cast(object, call_args.get("destination", "")))
                    minutes_raw = call_args.get("minutes_until_deadline", 30)
                    minutes = int(minutes_raw) if isinstance(minutes_raw, (int, str, float)) else 30
                    
                    # Create the persistent ticket IMMEDIATELY
                    l_load: dict[str, object] = {
                        "title": f"Trip to {destination[:40]}",
                        "origin": origin, "destination": destination,
                        "live": True,
                        "minutes_remaining": minutes,
                        "directions": {"steps": [], "total_duration": "Enriching...", "error": None}
                    }
                    created = await database.create_task(req.userid, "COMMUTE", json.dumps(l_load), status="in_focus")
                    if created:
                        task_uuid = str(cast(object, created["id"]))
                        # Register for scheduler
                        _ = commute_scheduler.register_live_trip(req.userid, origin, destination, minutes, task_id=task_uuid)
                        # Optionally enrich immediately too for faster UI feedback
                        _ = asyncio.create_task(enrich_commute_ticket(task_uuid, origin, destination))
                    _ = actions_taken.append("start_live_directions")

                elif call_name == "create_countdown":
                    c_title = str(cast(object, call_args.get("title", "Countdown")))
                    dur_raw = call_args.get("duration_minutes", 30)
                    c_duration = int(dur_raw) if isinstance(dur_raw, (int, str, float)) else 30
                    from datetime import datetime, timedelta, timezone
                    expires_at = datetime.now(timezone.utc) + timedelta(minutes=c_duration)
                    c_load = json.dumps({"title": c_title, "duration_minutes": c_duration, "expires_at": expires_at.isoformat()})
                    c_created = await database.create_task(userid=req.userid, type="COUNTDOWN", payload=c_load, status="idle")
                    if c_created:
                        ctid = str(cast(object, c_created["id"]))
                        async def _auto_delete(task_id: str, delay: int) -> None:
                            await asyncio.sleep(delay)
                            try: await database.delete_task(task_id)
                            except Exception: pass
                        _ = asyncio.create_task(_auto_delete(ctid, c_duration * 60))
                    _ = actions_taken.append("create_countdown")

            # Try to get text confirmation
            confirmation_text: str = "Action completed."
            try: 
                if response.text:
                    confirmation_text = response.text
            except Exception: pass
            
            # Broadcast WebSocket updates
            if actions_taken:
                await manager.broadcast_state_update(req.userid)
            return {"action": "multi" if len(actions_taken) > 1 else (actions_taken[0] if actions_taken else None), "response": confirmation_text}
        else:
            msg_text = "I'm sorry, I couldn't process that."
            try: msg_text = response.text
            except Exception: pass
            return {"action": "message", "response": msg_text}

    except Exception as e:
        import traceback
        print(f"Gemini Error: {e}")
        traceback.print_exc()
        return {"action": None}
