from fastapi import FastAPI, HTTPException, Depends, Header
from fastapi.staticfiles import StaticFiles
from fastapi.responses import HTMLResponse
from pydantic import BaseModel
import json
import asyncio
from contextlib import asynccontextmanager
from dotenv import load_dotenv
import database
from models import TaskCreate, UserCreate, UserLogin
import os
import ai_tools
import bcrypt
import commute_scheduler

load_dotenv(override=True)

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
    except:
        pass

app = FastAPI(lifespan=lifespan)

# Ensure static folder exists
os.makedirs("static", exist_ok=True)
app.mount("/static", StaticFiles(directory="static"), name="static")

@app.get("/", response_class=HTMLResponse)
async def read_root():
    index_path = os.path.join("static", "index.html")
    if os.path.exists(index_path):
        with open(index_path, "r") as f:
            return HTMLResponse(content=f.read())
    return HTMLResponse(content="<h1>Welcome</h1>")

@app.get("/api/tasks")
async def get_tasks(userid: int):
    try:
        tasks = await database.fetch_all_tasks(userid)
        for t in tasks:
            if isinstance(t.get('payload'), str):
                try:
                    t['payload'] = json.loads(t['payload'])
                except:
                    pass
        return tasks
    except Exception as e:
        raise HTTPException(status_code=500, detail=str(e))

@app.post("/api/auth/register")
async def register_user(user: UserCreate):
    try:
        existing = await database.get_user_by_name(user.first_name)
        if existing:
            raise HTTPException(status_code=400, detail="User already exists")
        
        hashed = get_password_hash(user.password)
        new_user = await database.create_user(user.first_name, user.last_name, hashed)
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
        # Pydantic enum `task.type.value` gets the string 'HABIT', 'TASK', etc.
        payload_str = json.dumps(task.payload) if task.payload else "{}"
        row = await database.create_task(task.userid, task.type.value, payload_str)
        if row:
            res = dict(row)
            if isinstance(res.get('payload'), str):
                try:
                    res['payload'] = json.loads(res['payload'])
                except:
                    pass
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
            except:
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
    You MUST ONLY use the provided tools to create, delete, edit, clear tickets, manage commutes, or get directions.

    CURRENT TICKET STACK (JSON):
    {context_str}

    TICKET CATEGORIES:
    - HABIT: Things that happen every day or routines (e.g., 'exercise', 'brush teeth', 'morning coffee').
    - TASK: Things that need to be done once, one-off actions (e.g., 'buy groceries').
    - EVENT: Meetings or time-specific appointments (e.g., 'dentist at 3pm').

    COMMUTE MANAGEMENT:
    - When the user describes a recurring trip (e.g., "I go from X to Y every morning at 9:30"), call `add_commute`.
    - Extract origin, destination, the arrival deadline in HH:MM format, a short label, and which days.
    - If no days are specified, default to weekdays (monday through friday).
    - The system will automatically send transit alerts 30 minutes before the deadline, every 5 minutes.
    - Existing commutes appear in the ticket stack above with type "COMMUTE".
    - To remove a commute, use `remove_commute` with the task_id.

    LIVE / URGENT DIRECTIONS:
    - When the user says "I need to get to X in N minutes" or "I'm at X, I need to be at Y by HH:MM", call `start_live_directions`.
    - This is for ONE-TIME urgent trips, NOT recurring commutes.
    - It will immediately show directions AND keep updating every 5 minutes until the deadline.
    - Calculate minutes_until_deadline from the user's phrasing (e.g. "in 30 mins" = 30, "by 17:00" = minutes from now to 17:00).

    DIRECTIONS:
    - When the user asks "how do I get from A to B?" or wants live directions NOW without a deadline, call `get_directions`.
    - This returns real-time transit routes (one-shot, no periodic updates).

    AGENT TASKS (background timers):
    - When the user mentions an AI agent, IDE, or tool working on something in the background, call `create_agent_task`.
    - Examples: "VSCode is working on...", "Cursor is fixing...", "Copilot is generating...", "The AI is analyzing..."
    - This creates a timed ticket with a countdown. It auto-deletes when the timer expires.
    - Default timer is 10 minutes. User can specify a different duration.

    PERSONAL COUNTDOWNS:
    - When the user says they need to do something within a certain time, call `create_countdown`.
    - Trigger phrases: "I need to ... in N minutes", "I have to ... in N min", "I should ... in 1 hour", etc.
    - Extract the action as the title and the duration in minutes.
    - This creates a COUNTDOWN ticket with a live timer. It auto-deletes when the timer expires.
    - IMPORTANT: This is different from a regular TASK. Use `create_countdown` when there is a specific time constraint ("in 30 min", "in 1 hour").

    MULTI-ACTION DECOMPOSITION (V7):
    - You are empowered to call MULTIPLE tools in a single turn if a user request is complex.
    - BREAK DOWN requests into discrete logical steps. 
    - Example: "Go grab the laundry detergent for my mum and kibble for my cat" 
      -> Call `create_ticket` for "Buy laundry detergent" (TASK)
      -> Call `create_ticket` for "Buy kibble for cat" (TASK)
      -> Call `create_ticket` for "Bring detergent to mum" (TASK)

    CONSISTENCY RULES:
    1. If the user asks to create a ticket that already exists or conflicts with an existing one, inform the user instead of calling a tool.
    2. When deleting, use the exact `id` from the context provided above.
    3. For 'clear everything', call `delete_all_tickets`.

    ACT AS A PURE ACTION MODEL:
    - Respond strictly with a brief, sexy confirmation of the actions taken.
    - If there is a conflict, explain it concisely.""",
            }
        )
        
        func_calls = getattr(response, 'function_calls', [])
        actions_taken = []
        directions_text = None
        if func_calls:
            for call in func_calls:
                if call.name == "create_ticket":
                    ticket_type = call.args.get("type", "TASK")
                    title = call.args.get("title", "Untitled")
                    payload_str = json.dumps({"title": title, "completed": False})
                    await database.create_task(req.userid, ticket_type, payload_str)
                    actions_taken.append("create")
                    
                elif call.name == "delete_ticket":
                    record_id = call.args.get("task_id", "")
                    try:
                        await database.delete_task(record_id)
                        actions_taken.append("delete")
                    except: pass

                elif call.name == "delete_all_tickets":
                    await database.delete_all_tasks(req.userid)
                    commute_scheduler.clear_user(req.userid)
                    actions_taken.append("clear")

                elif call.name == "edit_ticket":
                    tid = call.args.get("task_id", "")
                    new_type = call.args.get("type")
                    new_title = call.args.get("title")
                    
                    update_payload = None
                    if new_title:
                        update_payload = json.dumps({"title": new_title, "completed": False})
                    
                    await database.update_task(tid, new_type, update_payload)
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
                        raw = await ai_tools.call_directions_service(origin, destination)
                        parsed = ai_tools.parse_transit_directions(raw)
                        if parsed:
                            from datetime import datetime as dt
                            msg = commute_scheduler._format_commute_alert(
                                "Directions", parsed, "", dt.now()
                            )
                            directions_text = msg
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
                    except:
                        pass

                elif call.name == "start_live_directions":
                    origin = call.args.get("origin", "")
                    destination = call.args.get("destination", "")
                    minutes = int(call.args.get("minutes_until_deadline", 30))

                    # Register the live trip for periodic updates
                    trip_id = commute_scheduler.register_live_trip(
                        req.userid, origin, destination, minutes
                    )

                    # Also fetch directions immediately so the user gets instant feedback
                    try:
                        raw = await ai_tools.call_directions_service(origin, destination)
                        parsed = ai_tools.parse_transit_directions(raw)
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
            except:
                pass
            
            if not confirmation_text:
                confirmation_text = "Action completed."

            # If get_directions or start_live_directions was called, append the transit info
            if ("get_directions" in actions_taken or "start_live_directions" in actions_taken) and directions_text:
                confirmation_text = directions_text + "\n\n" + (confirmation_text or "")

            return {
                "action": "multi" if len(actions_taken) > 1 else (actions_taken[0] if actions_taken else None),
                "response": confirmation_text if actions_taken else "No changes made."
            }
        else:
            msg_text = None
            try:
                msg_text = response.text
            except:
                pass
            
            if not msg_text:
                msg_text = "I'm sorry, I couldn't process that."
            return {"action": "message", "response": msg_text}

    except Exception as e:
        import traceback
        print(f"Gemini Error: {e}")
        traceback.print_exc()
        return {"action": None}
