from fastapi import FastAPI, HTTPException, Depends, Header
from fastapi.staticfiles import StaticFiles
from fastapi.responses import HTMLResponse
from pydantic import BaseModel
import json
from contextlib import asynccontextmanager
from dotenv import load_dotenv
import database
from models import TaskCreate, UserCreate, UserLogin
import os
import ai_tools
import bcrypt

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
    yield
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
    You MUST ONLY use the provided tools to create, delete, edit, or clear tickets.

    CURRENT TICKET STACK (JSON):
    {context_str}

    TICKET CATEGORIES:
    - HABIT: Things that happen every day or routines (e.g., 'exercise', 'brush teeth', 'morning coffee').
    - TASK: Things that need to be done once, one-off actions (e.g., 'buy groceries').
    - EVENT: Meetings or time-specific appointments (e.g., 'dentist at 3pm').

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

            # Try to get text confirmation, fallback if not available
            confirmation_text = None
            try:
                confirmation_text = response.text
            except:
                pass
            
            if not confirmation_text:
                confirmation_text = "Action completed."

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
