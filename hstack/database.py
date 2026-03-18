import os
import asyncpg
import hashlib
import json
import uuid
from datetime import datetime
from typing import TYPE_CHECKING, Any

if TYPE_CHECKING:
    pass

# Ensure .env is loaded (handled by varlock)
DATABASE_URL: str | None = os.getenv("DATABASE_URL")

pool: asyncpg.Pool | None = None

async def connect_db() -> None:
    """
    Initialize the database connection pool.
    """
    global pool
    # Connect to the PostgreSQL database with asyncpg
    if not DATABASE_URL or "[YOUR-PASSWORD]" in DATABASE_URL:
        print("WARNING: Please update the [YOUR-PASSWORD] placeholder in your .env file!")
    display_url = DATABASE_URL.split('@')[-1] if DATABASE_URL else "UNKNOWN"
    print(f"Connecting to live database at: {display_url}")
    # Disable statement_cache_size to support Supabase pgbouncer transaction mode
    pool = await asyncpg.create_pool(DATABASE_URL, statement_cache_size=0)

async def close_db() -> None:
    """
    Close the database connection pool.
    """
    global pool
    if pool is not None:
        await pool.close()

async def create_user(first_name: str, last_name: str, password_hash: str) -> asyncpg.Record | None:
    """
    Create a new user in the database.
    
    Args:
        first_name: User's first name
        last_name: User's last name
        password_hash: Bcrypt hash of the password
        
    Returns:
        The created user record
    """
    if pool is None:
        raise Exception("Database not initialized")
    async with pool.acquire() as conn:
        return await conn.fetchrow(
            "INSERT INTO public.user (first_name, last_name, password) VALUES ($1, $2, $3) RETURNING id, first_name, last_name",
            first_name, last_name, password_hash
        )

async def get_user_by_name(first_name: str) -> asyncpg.Record | None:
    """
    Fetch a user by their first name.
    
    Args:
        first_name: The name to search for
        
    Returns:
        The user record if found
    """
    if pool is None:
        raise Exception("Database not initialized")
    async with pool.acquire() as conn:
        return await conn.fetchrow(
            "SELECT id, first_name, password FROM public.user WHERE first_name = $1",
            first_name
        )

async def fetch_all_tasks(userid: int) -> list[dict[str, object]]:
    """
    Fetch all tasks for a specific user.
    
    Args:
        userid: The user's ID
        
    Returns:
        A list of task dictionaries
    """
    if pool is None:
        raise Exception("Database not initialized")
    async with pool.acquire() as conn:
        records = await conn.fetch("SELECT * FROM public.task WHERE userid = $1 ORDER BY created_at ASC", userid)
        return [dict(record) for record in records]

async def create_task(userid: int | None = None, type: str = "TASK", payload: str = "{}", status: str = "idle") -> asyncpg.Record | None:
    """
    Create a new task record.
    
    Args:
        userid: Owner's ID
        type: Ticket type (HABIT, EVENT, etc.)
        payload: JSON blob of ticket data
        status: Initial status (idle, in_focus, etc.)
        
    Returns:
        The created task record
    """
    if pool is None:
        raise Exception("Database not initialized")
    async with pool.acquire() as conn:
        return await conn.fetchrow(
            "INSERT INTO public.task (userid, type, payload, status) VALUES ($1, $2, $3::json, $4) RETURNING *",
            userid, type, payload, status
        )

# For updating a task (e.g. marking it done via payload '{"completed": true}')
async def update_task_payload(task_id: str, payload: str) -> asyncpg.Record | None:
    """
    Update the JSON payload of a task.
    
    Args:
        task_id: The UUID of the task
        payload: The new JSON string payload
        
    Returns:
        The updated task record
    """
    if pool is None:
        raise Exception("Database not initialized")
    async with pool.acquire() as conn:
        return await conn.fetchrow(
            "UPDATE public.task SET payload = $1::json, updated_at = NOW() WHERE id = $2 RETURNING *",
            payload, task_id
        )

# Delete a task (Hard delete, logged as event)
async def delete_task(task_id: str) -> None:
    """
    Delete a task from the database.
    
    Args:
        task_id: The UUID of the task to delete
    """
    if pool is None:
        raise Exception("Database not initialized")
    async with pool.acquire() as conn:
        await conn.execute("DELETE FROM public.task WHERE id = $1", task_id)

# Batch Delete for User
async def delete_all_tasks(userid: int) -> None:
    """
    Delete all tasks for a specific user.
    
    Args:
        userid: The user's ID
    """
    if pool is None:
        raise Exception("Database not initialized")
    async with pool.acquire() as conn:
        await conn.execute("DELETE FROM public.task WHERE userid = $1", userid)

# Full Update for a task
async def update_task(
    task_id: str, 
    type: str | None = None, 
    payload: str | None = None, 
    status: str | None = None
) -> asyncpg.Record | None:
    """
    Perform a partial update on a task.
    
    Args:
        task_id: The UUID of the task
        type: Optional new type
        payload: Optional new JSON payload
        status: Optional new status
        
    Returns:
        The updated task record
    """
    if pool is None:
        raise Exception("Database not initialized")
    async with pool.acquire() as conn:
        updates: list[str] = []
        params: list[str] = []
        
        if type:
            params.append(type)
            updates.append(f"type = ${len(params)}")
        if payload:
            params.append(payload)
            updates.append(f"payload = ${len(params)}::json")
        if status:
            params.append(status)
            updates.append(f"status = ${len(params)}")
            
        if not updates:
            return await get_task(task_id)
            
        params.append(task_id)
        query = f"UPDATE public.task SET {', '.join(updates)}, updated_at = NOW() WHERE id = ${len(params)} RETURNING *"
        return await conn.fetchrow(query, *params)

async def update_task_status(task_id: str, status: str) -> asyncpg.Record | None:
    """
    Update only the status of a task.
    
    Args:
        task_id: The UUID of the task
        status: The new status string
        
    Returns:
        The updated task record
    """
    if pool is None:
        raise Exception("Database not initialized")
    async with pool.acquire() as conn:
        return await conn.fetchrow(
            "UPDATE public.task SET status = $1, updated_at = NOW() WHERE id = $2 RETURNING *",
            status, task_id
        )

async def get_task(task_id: str) -> asyncpg.Record | None:
    """
    Fetch a single task by its ID.
    
    Args:
        task_id: The UUID of the task
        
    Returns:
        The task record if found
    """
    if pool is None:
        raise Exception("Database not initialized")
    async with pool.acquire() as conn:
        return await conn.fetchrow("SELECT * FROM public.task WHERE id = $1", task_id)


async def calculate_state_hash(userid: int) -> str:
    """
    Calculate a deterministic hash of the user's FULL task stack.
    
    Args:
        userid: The user's ID
        
    Returns:
        Hex-encoded SHA256 hash
    """
    if pool is None:
        raise Exception("Database not initialized")
    async with pool.acquire() as conn:
        # Fetch all tasks linearly ordered
        records = await conn.fetch(
            "SELECT id, type, payload, status FROM public.task WHERE userid = $1 ORDER BY created_at ASC, id ASC",
            userid
        )
        
        state_list: list[dict[str, object]] = []
        for r in records:
            d: dict[str, object] = dict(r)
            d["id"] = str(d["id"])
            payload_raw = d.get("payload")
            if isinstance(payload_raw, str):
                try:
                    d["payload"] = json.loads(payload_raw)
                except Exception:
                    pass
            state_list.append(d)
        
        # Serialize to JSON deterministically
        state_str = json.dumps(state_list, sort_keys=True, separators=(',', ':'))
        return hashlib.sha256(state_str.encode('utf-8')).hexdigest()


async def save_sync_event(
    userid: int,
    action_id: uuid.UUID | str,
    action_type: str,
    entity_id: uuid.UUID | str,
    entity_type: str,
    payload: dict[str, object] | None
) -> None:
    """
    Log an event directly into the sync_events table.
    
    Args:
        userid: User ID
        action_id: Unique event ID
        action_type: CREATE/UPDATE/DELETE
        entity_id: ID of the modified entity
        entity_type: Type of the modified entity
        payload: Event data
    """
    if pool is None:
        raise Exception("Database not initialized")
        
    payload_str = json.dumps(payload) if payload else "{}"
    
    async with pool.acquire() as conn:
        await conn.execute(
            """
            INSERT INTO public.sync_events 
            (userid, action_id, type, entity_id, entity_type, payload)
            VALUES ($1, $2, $3, $4, $5, $6::jsonb)
            ON CONFLICT (action_id) DO NOTHING
            """,
            userid, str(action_id), action_type, str(entity_id), entity_type, payload_str
        )

async def get_sync_events(userid: int, after_id: int = 0) -> list[dict[str, object]]:
    """
    Fetch events for a user that occurred after a specific event ID for sequential syncing.
    
    Args:
        userid: User ID
        after_id: Last processed event ID
        
    Returns:
        List of event dictionaries
    """
    if pool is None:
        raise Exception("Database not initialized")
    async with pool.acquire() as conn:
        records = await conn.fetch(
            "SELECT id, action_id, type, entity_id, entity_type, payload, timestamp FROM public.sync_events "
            "WHERE userid = $1 AND id > $2 ORDER BY id ASC",
            userid, after_id
        )
        events: list[dict[str, object]] = []
        for r in records:
            d: dict[str, object] = dict(r)
            d["action_id"] = str(d["action_id"])
            d["entity_id"] = str(d["entity_id"])
            payload_raw = d.get("payload")
            if isinstance(payload_raw, str):
                try:
                    d["payload"] = json.loads(payload_raw)
                except Exception:
                    pass
            # Convert timestamp to iso string
            ts = d.get("timestamp")
            if isinstance(ts, datetime):
                d["timestamp"] = ts.isoformat()
            events.append(d)
        return events

