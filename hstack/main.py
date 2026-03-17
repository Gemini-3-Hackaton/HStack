from fastapi import FastAPI, HTTPException, WebSocket, WebSocketDisconnect
from fastapi.middleware.cors import CORSMiddleware
from fastapi.responses import HTMLResponse
from pydantic import BaseModel
import json
from typing import Any
import asyncio
from contextlib import asynccontextmanager
from . import database
from .models import TaskCreate, UserCreate, UserLogin
import os
from . import ai_tools
import bcrypt
from . import commute_scheduler

def verify_password(plain_password, hashed_password):
    try:
        return bcrypt.checkpw(
            plain_password[:72].encode('utf-8'),
            hashed_password.encode('utf-8')
        )
    except Exception:
        return False

def get_password_hash(password):
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
    # Start the commute scheduler as a background task
    scheduler_task = asyncio.create_task(commute_scheduler.run_scheduler())
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
                        payload = action.get("payload", {})
                        
                        try:
                            # 1. Log Event to event store
                            await database.save_sync_event(
                                userid=userid,
                                action_id=action_id,
                                action_type=action_type,
                                entity_id=entity_id,
                                entity_type=entity_type,
                                payload=payload
                            )

                            # 2. Apply Event to Task table (Materialized View style)
                            if action_type == "CREATE":
                                await database.create_task(userid=userid, type=entity_type, payload=json.dumps(payload))
                            elif action_type == "UPDATE":
                                await database.update_task_payload(task_id=entity_id, payload=json.dumps(payload))
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
                    t['payload'] = json.loads(t['payload'])
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
        row = await database.create_task(task.userid, task.type.value, payload_str)
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

class ChatRequest(BaseModel):
    message: str
    userid: int


# ── Commute alerts endpoint ──────────────────────────────────────────
@app.get("/api/commute-alerts/{userid}")
async def get_commute_alerts(userid: int):
    """Poll this endpoint to get pending commute direction alerts for a user."""
    alerts = commute_scheduler.get_alerts(userid)
    return {"alerts": alerts}


@app.get("/api/commutes/{userid}")
async def get_user_commutes(userid: int):
    """List all registered commutes for a user."""
    if database.pool is None:
        raise HTTPException(status_code=500, detail="Database not connected")
    async with database.pool.acquire() as conn:
        rows = await conn.fetch(
            "SELECT * FROM public.task WHERE userid = $1 AND type = 'COMMUTE' ORDER BY created_at ASC",
            userid,
        )
    commutes = []
    for r in rows:
        d = dict(r)
        if isinstance(d.get("payload"), str):
            try:
                d["payload"] = json.loads(d["payload"])
            except Exception:
                pass
        commutes.append(d)
    return commutes


@app.get("/api/live-trips/{userid}")
async def get_live_trips(userid: int):
    """List active live/urgent direction trips for a user."""
    return {"trips": commute_scheduler.get_active_live_trips(userid)}


@app.post("/api/chat")
async def chat_with_gemini(req: ChatRequest):
    if not ai_tools.client:
        return {"action": "message", "response": "Warning: GEMINI_API_KEY is not set in `.env`."}

    # 1. Fetch current context for the user
    current_tickets = await database.fetch_all_tasks(req.userid)
    
    # Custom encoder for UUID and datetime
    def custom_encoder(obj):
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
    3. Respond with a brief, sexy confirmation of actions taken.""",
            }
        )
        
        # Extract function calls from the response
        func_calls = []
        try:
            if response.candidates:
                for part in response.candidates[0].content.parts:
                    if part.function_call:
                        func_calls.append(part.function_call)
        except Exception:
            # Fallback for different SDK versions or properties
            func_calls = getattr(response, 'function_calls', [])

        actions_taken = []
        directions_text = None
        if func_calls:
            for call in func_calls:
                if call.name == "create_ticket":
                    ticket_type = call.args.get("type", "TASK")
                    
                    # Capture all other args into the payload
                    p_load = {"completed": False}
                    for key, val in call.args.items():
                        if key != "type":
                            p_load[key] = val
                            
                    await database.create_task(req.userid, ticket_type, json.dumps(p_load))
                    actions_taken.append("create")
                    
                elif call.name == "delete_ticket":
                    record_id = call.args.get("task_id", "")
                    try:
                        await database.delete_task(record_id)
                        actions_taken.append("delete")
                    except Exception:
                        pass

                elif call.name == "delete_all_tickets":
                    await database.delete_all_tasks(req.userid)
                    commute_scheduler.clear_user(req.userid)
                    actions_taken.append("clear")

                elif call.name == "edit_ticket":
                    tid = call.args.get("task_id", "")
                    new_type = call.args.get("type")
                    new_title = call.args.get("title")
                    new_time = call.args.get("scheduled_time")
                    new_dur = call.args.get("duration_minutes")
                    
                    # Fetch current payload to merge
                    existing_task = await database.get_task(tid)
                    curr_payload = {}
                    if existing_task and existing_task.get("payload"):
                        try:
                            curr_payload = json.loads(existing_task["payload"])
                        except Exception:
                            pass
                    
                    if new_title: curr_payload["title"] = new_title
                    if new_time: curr_payload["scheduled_time"] = new_time
                    if new_dur: curr_payload["duration_minutes"] = new_dur
                    
                    await database.update_task(tid, new_type, json.dumps(curr_payload))
                    actions_taken.append("edit")

                elif call.name == "add_commute":
                    label = call.args.get("label", "commute")
                    origin = call.args.get("origin", "")
                    destination = call.args.get("destination", "")
                    deadline = call.args.get("deadline", "09:00")
                    days = call.args.get("days", "monday,tuesday,wednesday,thursday,friday")

                    commute_payload = json.dumps({
                        "title": f"🚇 {label.replace('_', ' ').title()}: {origin[:30]}… → {destination[:30]}… @ {deadline}",
                        "label": label,
                        "origin": origin,
                        "destination": destination,
                        "deadline": deadline,
                        "days": days,
                        "completed": False,
                    })
                    await database.create_task(req.userid, "COMMUTE", commute_payload)
                    actions_taken.append("add_commute")

                elif call.name == "get_directions":
                    origin = call.args.get("origin", "")
                    destination = call.args.get("destination", "")
                    try:
                        raw_resp = await ai_tools.call_directions_service(origin, destination)
                        raw_routes = raw_resp.get("routes", []) if isinstance(raw_resp, dict) else []
                        parsed = ai_tools.parse_transit_directions(raw_routes)
                        if parsed:
                            from datetime import datetime as dt
                            msg = commute_scheduler._format_commute_alert(
                                "Directions", parsed, "", dt.now()
                            )
                        else:
                            directions_text = "No transit routes found for that journey."
                    except Exception as exc:
                        directions_text = f"Could not fetch directions: {exc}"
                    actions_taken.append("get_directions")

                elif call.name == "remove_commute":
                    record_id = call.args.get("task_id", "")
                    try:
                        await database.delete_task(record_id)
                        actions_taken.append("remove_commute")
                    except Exception:
                        pass

                elif call.name == "start_live_directions":
                    origin = call.args.get("origin", "")
                    destination = call.args.get("destination", "")
                    minutes = int(call.args.get("minutes_until_deadline", 30))

                    # Register the live trip for periodic updates
                    commute_scheduler.register_live_trip(
                        req.userid, origin, destination, minutes
                    )

                    # Also fetch directions immediately so the user gets instant feedback
                    try:
                        raw_resp = await ai_tools.call_directions_service(origin, destination)
                        raw_routes = raw_resp.get("routes", []) if isinstance(raw_resp, dict) else []
                        parsed = ai_tools.parse_transit_directions(raw_routes)
                        if parsed:
                            from datetime import datetime as dt, timedelta, timezone
                            now = dt.now()
                            deadline_dt = now + timedelta(minutes=minutes)
                            deadline_str = deadline_dt.strftime("%H:%M")
                            msg = commute_scheduler._format_commute_alert(
                                f"🚨 Trip to {destination[:40]}", parsed, deadline_str, now
                            )
                            directions_text = f"⏳ {minutes} min until deadline\n{msg}\n\n📡 Live tracking started – updates every 5 min."
                        else:
                            directions_text = f"No transit routes found. Live tracking started – will retry every 5 min for {minutes} min."
                    except Exception as exc:
                        directions_text = f"Could not fetch initial directions: {exc}\n📡 Live tracking started – will retry every 5 min."
                    actions_taken.append("start_live_directions")

                elif call.name == "create_agent_task":
                    title = call.args.get("title", "Agent task")
                    duration = int(call.args.get("duration_minutes", 10))
                    from datetime import datetime, timedelta, timezone
                    expires_at = datetime.now(timezone.utc) + timedelta(minutes=duration)
                    payload = json.dumps({
                        "title": title,
                        "agent_task": True,
                        "duration_minutes": duration,
                        "expires_at": expires_at.isoformat()
                    })
                    created = await database.create_task(
                        userid=req.userid,
                        type="AGENT_TASK",
                        payload=payload
                    )
                    if created:
                        task_id = created["id"]
                        # Schedule auto-deletion when the timer expires
                        async def _auto_delete(tid, delay):
                            await asyncio.sleep(delay)
                            try:
                                await database.delete_task(tid)
                            except Exception:
                                pass
                        asyncio.create_task(_auto_delete(task_id, duration * 60))
                    actions_taken.append("create_agent_task")

                elif call.name == "create_countdown":
                    title = call.args.get("title", "Countdown")
                    duration = int(call.args.get("duration_minutes", 30))
                    from datetime import datetime, timedelta, timezone
                    expires_at = datetime.now(timezone.utc) + timedelta(minutes=duration)
                    payload = json.dumps({
                        "title": title,
                        "countdown": True,
                        "duration_minutes": duration,
                        "expires_at": expires_at.isoformat()
                    })
                    created = await database.create_task(
                        userid=req.userid,
                        type="COUNTDOWN",
                        payload=payload
                    )
                    if created:
                        task_id = created["id"]
                        async def _auto_delete_cd(tid, delay):
                            await asyncio.sleep(delay)
                            try:
                                await database.delete_task(tid)
                            except Exception:
                                pass
                        asyncio.create_task(_auto_delete_cd(task_id, duration * 60))
                    actions_taken.append("create_countdown")

            # Try to get text confirmation, fallback if not available
            confirmation_text = None
            try:
                confirmation_text = response.text
            except Exception:
                pass
            
            if not confirmation_text:
                confirmation_text = "Action completed."

            # If get_directions or start_live_directions was called, append the transit info
            if ("get_directions" in actions_taken or "start_live_directions" in actions_taken) and directions_text:
                confirmation_text = directions_text + "\n\n" + (confirmation_text or "")

            # Broadcast WebSocket updates if tasks changed
            if any(a in actions_taken for a in ["create", "delete", "clear", "edit", "add_commute", "remove_commute", "create_agent_task", "create_countdown"]):
                await manager.broadcast_state_update(req.userid)

            return {
                "action": "multi" if len(actions_taken) > 1 else (actions_taken[0] if actions_taken else None),
                "response": confirmation_text if actions_taken else "No changes made."
            }
        else:
            msg_text = None
            try:
                msg_text = response.text
            except Exception:
                pass
            
            if not msg_text:
                msg_text = "I'm sorry, I couldn't process that."
            return {"action": "message", "response": msg_text}

    except Exception as e:
        import traceback
        print(f"Gemini Error: {e}")
        traceback.print_exc()
        return {"action": None}
