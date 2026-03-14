from pydantic import BaseModel, Field
from typing import Optional, Any, Dict
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
    userid: Optional[int] = None
    type: TicketType = TicketType.TASK
    payload: Optional[Any] = None

class TaskCreate(TaskBase):
    pass

class UserCreate(BaseModel):
    first_name: str
    last_name: str
    password: str

class UserLogin(BaseModel):
    first_name: str
    password: str

class TaskModel(TaskBase):
    id: UUID
    created_at: datetime

class UserBase(BaseModel):
    first_name: Optional[str] = None
    last_name: Optional[str] = None

class UserCreate(UserBase):
    password: Optional[str] = None

class UserModel(UserBase):
    id: int
    created_at: datetime
