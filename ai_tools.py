import os
from dotenv import load_dotenv
from google import genai
from google.genai import types

load_dotenv()

# Initialize GenAI Client
GEMINI_API_KEY = os.getenv("GEMINI_API_KEY")
client = genai.Client(api_key=GEMINI_API_KEY) if GEMINI_API_KEY else None

# Mock data mapping for when Supabase DB is unreachable
from database import fetch_all_tasks, create_task, delete_task
import database

# Defines the function schemas to pass to the model
create_ticket_function_schema = {
    "name": "create_ticket",
    "description": "Create a new ticket in the user's stack. Must specify the type of ticket (HABIT, EVENT, or TASK) and the title payload.",
    "parameters": {
        "type": "OBJECT",
        "properties": {
            "type": {
                "type": "STRING",
                "description": "The type of the ticket. MUST be exactly one of: HABIT, EVENT, TASK"
            },
            "title": {
                "type": "STRING",
                "description": "The title or description of the ticket"
            }
        },
        "required": ["type", "title"]
    }
}

delete_ticket_function_schema = {
    "name": "delete_ticket",
    "description": "Delete a ticket from the user's stack given its ID string.",
    "parameters": {
        "type": "OBJECT",
        "properties": {
            "task_id": {
                "type": "STRING",
                "description": "The exact ID of the task/ticket to delete"
            }
        },
        "required": ["task_id"]
    }
}

delete_all_tickets_function_schema = {
    "name": "delete_all_tickets",
    "description": "Deletes the entire stack of tickets for the user. Use this when the user wants to 'clear everything' or 'get rid of all tickets'.",
    "parameters": {
        "type": "OBJECT",
        "properties": {}
    }
}

edit_ticket_function_schema = {
    "name": "edit_ticket",
    "description": "Edit an existing ticket in the user's stack. You can change its type or its title payload.",
    "parameters": {
        "type": "OBJECT",
        "properties": {
            "task_id": {
                "type": "STRING",
                "description": "The ID of the ticket to edit"
            },
            "type": {
                "type": "STRING",
                "description": "The new type (HABIT, EVENT, or TASK). Skip if no change."
            },
            "title": {
                "type": "STRING",
                "description": "The new title/description. Skip if no change."
            }
        },
        "required": ["task_id"]
    }
}

# The single combined tool definition for GenAI
chat_tools = [
     {"function_declarations": [
         create_ticket_function_schema, 
         delete_ticket_function_schema,
         delete_all_tickets_function_schema,
         edit_ticket_function_schema
     ]}
]
