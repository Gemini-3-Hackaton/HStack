from typing import Any
from pydantic import BaseModel
from datetime import datetime
from uuid import UUID
from enum import Enum

class TicketType(str, Enum):
    HABIT = "HABIT"
    EVENT = "EVENT"
    TASK = "TASK"
    COMMUTE = "COMMUTE"
    AGENT_TASK = "AGENT_TASK"
    COUNTDOWN = "COUNTDOWN"

class TaskBase(BaseModel):
    userid: int | None = None
    type: TicketType = TicketType.TASK
    payload: dict[str, Any] | None = None

class TaskCreate(TaskBase):
    pass



class UserLoginBase(BaseModel):
    first_name: str
    password: str

class UserLogin(UserLoginBase):
    pass

class TaskModel(TaskBase):
    id: UUID
    created_at: datetime
    updated_at: datetime
    deleted_at: datetime | None = None

class UserBase(BaseModel):
    first_name: str | None = None
    last_name: str | None = None

class UserCreate(UserBase):
    password: str | None = None

class UserModel(UserBase):
    id: int
    created_at: datetime

# Sync Models
class SyncActionType(str, Enum):
    CREATE = "CREATE"
    UPDATE = "UPDATE"
    DELETE = "DELETE"

class SyncAction(BaseModel):
    action_id: UUID
    type: SyncActionType
    entity_id: UUID
    entity_type: str = "TASK"
    payload: dict[str, Any] | None = None
    timestamp: datetime

class SyncHandshake(BaseModel):
    client_hash: str

class SyncResponse(BaseModel):
    server_hash: str
    actions: list[SyncAction] = []
    ack_action_ids: list[UUID] = []
