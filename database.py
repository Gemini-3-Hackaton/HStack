import os
import asyncpg
from typing import Optional
from dotenv import load_dotenv

# Ensure .env is loaded before we try to read DATABASE_URL
load_dotenv(override=True)

DATABASE_URL = os.getenv("DATABASE_URL")

pool: Optional[asyncpg.Pool] = None

async def connect_db():
    global pool
    # Connect to the PostgreSQL database with asyncpg
    if not DATABASE_URL or "[YOUR-PASSWORD]" in DATABASE_URL:
        print("WARNING: Please update the [YOUR-PASSWORD] placeholder in your .env file!")
    print(f"Connecting to live database at: {DATABASE_URL.split('@')[-1]}")
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

async def create_task(userid: Optional[int] = None, type: str = "TASK", payload: str = "{}"):
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
            "UPDATE public.task SET payload = $1::json WHERE id = $2 RETURNING *",
            payload, task_id
        )

# Delete a task
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
async def update_task(task_id: str, type: Optional[str] = None, payload: Optional[str] = None):
    if pool is None:
        raise Exception("Database not initialized")
    async with pool.acquire() as conn:
        if type and payload:
            return await conn.fetchrow(
                "UPDATE public.task SET type = $1, payload = $2::json WHERE id = $3 RETURNING *",
                type, payload, task_id
            )
        elif type:
            return await conn.fetchrow(
                "UPDATE public.task SET type = $1 WHERE id = $2 RETURNING *",
                type, task_id
            )
        elif payload:
            return await conn.fetchrow(
                "UPDATE public.task SET payload = $1::json WHERE id = $2 RETURNING *",
                payload, task_id
            )


