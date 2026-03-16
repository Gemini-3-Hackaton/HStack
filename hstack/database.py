import os
import asyncpg  # pyright: ignore[reportMissingTypeStubs]
import hashlib
import json
import uuid
from typing import Any

# Ensure .env is loaded (handled by varlock)
DATABASE_URL = os.getenv("DATABASE_URL")

pool: asyncpg.Pool | None = None

async def connect_db():
    global pool
    # Connect to the PostgreSQL database with asyncpg
    if not DATABASE_URL or "[YOUR-PASSWORD]" in DATABASE_URL:
        print("WARNING: Please update the [YOUR-PASSWORD] placeholder in your .env file!")
    display_url = DATABASE_URL.split('@')[-1] if DATABASE_URL else "UNKNOWN"
    print(f"Connecting to live database at: {display_url}")
    # Disable statement_cache_size to support Supabase pgbouncer transaction mode
    pool = await asyncpg.create_pool(DATABASE_URL, statement_cache_size=0)

async def close_db():
    global pool
    if pool is not None:
        await pool.close()

async def create_user(first_name: str, last_name: str, password_hash: str):
    if pool is None:
        raise Exception("Database not initialized")
    async with pool.acquire() as conn:
        return await conn.fetchrow(
            "INSERT INTO public.user (first_name, last_name, password) VALUES ($1, $2, $3) RETURNING id, first_name, last_name",
            first_name, last_name, password_hash
        )

async def get_user_by_name(first_name: str):
    if pool is None:
        raise Exception("Database not initialized")
    async with pool.acquire() as conn:
        return await conn.fetchrow(
            "SELECT id, first_name, password FROM public.user WHERE first_name = $1",
            first_name
        )

async def fetch_all_tasks(userid: int):
    if pool is None:
        raise Exception("Database not initialized")
    async with pool.acquire() as conn:
        records = await conn.fetch("SELECT * FROM public.task WHERE userid = $1 ORDER BY created_at ASC", userid)
        return [dict(record) for record in records]

async def create_task(userid: int | None = None, type: str = "TASK", payload: str = "{}"):
    if pool is None:
        raise Exception("Database not initialized")
    async with pool.acquire() as conn:
        return await conn.fetchrow(
            "INSERT INTO public.task (userid, type, payload) VALUES ($1, $2, $3::json) RETURNING *",
            userid, type, payload 
        )

# For updating a task (e.g. marking it done via payload '{"completed": true}')
async def update_task_payload(task_id: str, payload: str):
    if pool is None:
        raise Exception("Database not initialized")
    async with pool.acquire() as conn:
        return await conn.fetchrow(
            "UPDATE public.task SET payload = $1::json, updated_at = NOW() WHERE id = $2 RETURNING *",
            payload, task_id
        )

# Delete a task (Hard delete, logged as event)
async def delete_task(task_id: str):
    if pool is None:
        raise Exception("Database not initialized")
    async with pool.acquire() as conn:
        await conn.execute("DELETE FROM public.task WHERE id = $1", task_id)

# Batch Delete for User
async def delete_all_tasks(userid: int):
    if pool is None:
        raise Exception("Database not initialized")
    async with pool.acquire() as conn:
        await conn.execute("DELETE FROM public.task WHERE userid = $1", userid)

# Full Update for a task
async def update_task(task_id: str, type: str | None = None, payload: str | None = None):
    if pool is None:
        raise Exception("Database not initialized")
    async with pool.acquire() as conn:
        if type and payload:
            return await conn.fetchrow(
                "UPDATE public.task SET type = $1, payload = $2::json, updated_at = NOW() WHERE id = $3 RETURNING *",
                type, payload, task_id
            )
        elif type:
            return await conn.fetchrow(
                "UPDATE public.task SET type = $1, updated_at = NOW() WHERE id = $2 RETURNING *",
                type, task_id
            )
        elif payload:
            return await conn.fetchrow(
                "UPDATE public.task SET payload = $1::json, updated_at = NOW() WHERE id = $2 RETURNING *",
                payload, task_id
            )


async def calculate_state_hash(userid: int) -> str:
    """Calculate a deterministic hash of the user's FULL task stack."""
    if pool is None:
        raise Exception("Database not initialized")
    async with pool.acquire() as conn:
        # Fetch all tasks linearly ordered
        records = await conn.fetch(
            "SELECT id, type, payload FROM public.task WHERE userid = $1 ORDER BY created_at ASC, id ASC",
            userid
        )
        
        state_list = []
        for r in records:
            d = dict(r)
            d["id"] = str(d["id"])
            if isinstance(d.get("payload"), str):
                try:
                    d["payload"] = json.loads(d["payload"])
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
    payload: dict[str, Any]
) -> None:
    """Log an event directly into the sync_events table."""
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

async def get_sync_events(userid: int, after_id: int = 0) -> list[dict[str, Any]]:
    """Fetch events for a user that occurred after a specific event ID for sequential syncing."""
    if pool is None:
        raise Exception("Database not initialized")
    async with pool.acquire() as conn:
        records = await conn.fetch(
            "SELECT id, action_id, type, entity_id, entity_type, payload, timestamp FROM public.sync_events WHERE userid = $1 AND id > $2 ORDER BY id ASC",
            userid, after_id
        )
        events = []
        for r in records:
            d = dict(r)
            d["action_id"] = str(d["action_id"])
            d["entity_id"] = str(d["entity_id"])
            if isinstance(d.get("payload"), str):
                try:
                    d["payload"] = json.loads(d["payload"])
                except Exception:
                    pass
            # Convert timestamp to iso string
            if d.get("timestamp"):
                d["timestamp"] = d["timestamp"].isoformat()
            events.append(d)
        return events

