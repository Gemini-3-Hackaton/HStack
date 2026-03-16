from typing import Any
import os
import httpx
from google import genai

# Initialize GenAI Client
GEMINI_API_KEY = os.getenv("GEMINI_API_KEY")
client = genai.Client(api_key=GEMINI_API_KEY) if GEMINI_API_KEY else None

# Directions service URL (the microservice running on port 8001)
DIRECTIONS_SERVICE_URL = os.getenv("DIRECTIONS_SERVICE_URL", "http://localhost:8001")

# Mock data mapping for when Supabase DB is unreachable


# --------------- Directions helper ---------------
async def call_directions_service(origin: str, destination: str) -> dict[str, Any]:
    """Call the directions microservice and return parsed transit info."""
    origin = origin.strip()
    destination = destination.strip()
    if not origin or not destination:
        raise ValueError("Both origin and destination are required.")

    async with httpx.AsyncClient(timeout=30.0) as client_http:
        try:
            resp = await client_http.post(
                f"{DIRECTIONS_SERVICE_URL}/directions",
                json={"origin": origin, "destination": destination},
            )
        except httpx.TimeoutException as exc:
            raise RuntimeError("Directions service timed out.") from exc
        except httpx.HTTPError as exc:
            raise RuntimeError(
                f"Directions service is unreachable at {DIRECTIONS_SERVICE_URL}."
            ) from exc

    if resp.is_error:
        detail = None
        try:
            payload = resp.json()
        except ValueError:
            payload = None
        if isinstance(payload, dict):
            detail = payload.get("detail")
        raise RuntimeError(detail or f"Directions service returned HTTP {resp.status_code}.")

    return resp.json()


def parse_transit_directions(raw_routes: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """Parse raw Google Maps directions response into a clean list of route summaries."""
    parsed = []
    for route in raw_routes:
        leg = route.get("legs", [{}])[0]
        steps_info = []
        for step in leg.get("steps", []):
            travel_mode = step.get("travel_mode", "")
            info = {
                "travel_mode": travel_mode,
                "duration": step.get("duration", {}).get("text", ""),
                "instruction": step.get("html_instructions", ""),
            }
            transit = step.get("transit_details")
            if transit:
                line = transit.get("line", {})
                info["transit_line"] = line.get("short_name") or line.get("name", "")
                info["vehicle_type"] = line.get("vehicle", {}).get("type", "")
                info["departure_stop"] = transit.get("departure_stop", {}).get("name", "")
                info["arrival_stop"] = transit.get("arrival_stop", {}).get("name", "")
                dep_time = transit.get("departure_time", {})
                arr_time = transit.get("arrival_time", {})
                info["departure_time"] = dep_time.get("text", "")
                info["departure_timestamp"] = dep_time.get("value")
                info["arrival_time"] = arr_time.get("text", "")
                info["num_stops"] = transit.get("num_stops", 0)
            steps_info.append(info)

        parsed.append({
            "summary": route.get("summary", ""),
            "total_duration": leg.get("duration", {}).get("text", ""),
            "total_duration_sec": leg.get("duration", {}).get("value", 0),
            "departure_time": leg.get("departure_time", {}).get("text", ""),
            "arrival_time": leg.get("arrival_time", {}).get("text", ""),
            "steps": steps_info,
        })
    # Sort by total duration
    parsed.sort(key=lambda r: r["total_duration_sec"])
    return parsed

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

# --------------- Commute tools ---------------

add_commute_function_schema = {
    "name": "add_commute",
    "description": """Register a recurring commute for the user. Use this when the user says they regularly travel from one place to another at a specific time (e.g., 'I go from X to Y every morning at 9:30').
    This will create a scheduled commute that automatically provides transit directions before the deadline.""",
    "parameters": {
        "type": "OBJECT",
        "properties": {
            "label": {
                "type": "STRING",
                "description": "A short label for the commute, e.g. 'morning_commute', 'evening_commute', 'work_commute'"
            },
            "origin": {
                "type": "STRING",
                "description": "The full starting address or place name"
            },
            "destination": {
                "type": "STRING",
                "description": "The full destination address or place name"
            },
            "deadline": {
                "type": "STRING",
                "description": "The time the user needs to arrive, in HH:MM 24-hour format (e.g. '09:30', '18:00')"
            },
            "days": {
                "type": "STRING",
                "description": "Comma-separated days of the week this commute applies, e.g. 'monday,tuesday,wednesday,thursday,friday'. Default is weekdays."
            }
        },
        "required": ["label", "origin", "destination", "deadline"]
    }
}

get_directions_function_schema = {
    "name": "get_directions",
    "description": "Get real-time transit directions between two places right now. Use this when the user asks how to get from A to B, or wants to know the fastest route.",
    "parameters": {
        "type": "OBJECT",
        "properties": {
            "origin": {
                "type": "STRING",
                "description": "The starting address or place name"
            },
            "destination": {
                "type": "STRING",
                "description": "The destination address or place name"
            }
        },
        "required": ["origin", "destination"]
    }
}

remove_commute_function_schema = {
    "name": "remove_commute",
    "description": "Remove/delete a registered commute by its task ID.",
    "parameters": {
        "type": "OBJECT",
        "properties": {
            "task_id": {
                "type": "STRING",
                "description": "The ID of the commute task to remove"
            }
        },
        "required": ["task_id"]
    }
}

start_live_directions_function_schema = {
    "name": "start_live_directions",
    "description": """Start a live directions tracker when the user has an URGENT or ONE-TIME trip with a deadline.
Use this when the user says things like:
- 'I need to get to X in 30 minutes'
- 'I am at X and I need to be at Y by 17:00'
- 'I'm currently at X, I need to go to Y in 20 mins'
This will immediately fetch directions AND keep updating every 5 minutes with fresh transit info until the deadline passes.
Do NOT use add_commute for this – add_commute is for recurring/daily commutes only.""",
    "parameters": {
        "type": "OBJECT",
        "properties": {
            "origin": {
                "type": "STRING",
                "description": "The user's current location / starting address"
            },
            "destination": {
                "type": "STRING",
                "description": "Where the user needs to go"
            },
            "minutes_until_deadline": {
                "type": "INTEGER",
                "description": "How many minutes from now the user needs to arrive. e.g. if they say 'in 30 mins' this is 30."
            }
        },
        "required": ["origin", "destination", "minutes_until_deadline"]
    }
}

create_agent_task_function_schema = {
    "name": "create_agent_task",
    "description": """Create a timed background agent task. Use this when the user mentions an AI agent, IDE, or automated tool doing work in the background.
Examples:
- 'VSCode is working on refactoring my code'
- 'Cursor is fixing the tests'
- 'Copilot is generating the migration'
- 'The AI is analyzing the codebase'
- 'Claude is reviewing the PR'
This creates a AGENT_TASK ticket with a 10-minute countdown timer. The ticket auto-deletes when the timer expires.""",
    "parameters": {
        "type": "OBJECT",
        "properties": {
            "title": {
                "type": "STRING",
                "description": "A short description of what the agent is doing, e.g. 'VSCode refactoring auth module'"
            },
            "duration_minutes": {
                "type": "INTEGER",
                "description": "How many minutes the timer should run. Default is 10."
            }
        },
        "required": ["title"]
    }
}

create_countdown_function_schema = {
    "name": "create_countdown",
    "description": """Create a personal countdown timer. Use this when the user says they need to do something within a certain time.
Examples:
- 'I need to eat in 30 minutes'
- 'I have to leave in 15 min'
- 'Remind me to call mum in 1 hour'
- 'I should start cooking in 20 minutes'
- 'I have a meeting in 45 min'
This creates a COUNTDOWN ticket with a live timer that auto-deletes when it expires.""",
    "parameters": {
        "type": "OBJECT",
        "properties": {
            "title": {
                "type": "STRING",
                "description": "A short description of what the user needs to do, e.g. 'Time to eat'"
            },
            "duration_minutes": {
                "type": "INTEGER",
                "description": "Number of minutes until the deadline. Extract from phrases like 'in 30 min', 'in 1 hour' (=60), etc."
            }
        },
        "required": ["title", "duration_minutes"]
    }
}

# The single combined tool definition for GenAI
chat_tools = [
     {"function_declarations": [
         create_ticket_function_schema,
         delete_ticket_function_schema,
         delete_all_tickets_function_schema,
         edit_ticket_function_schema,
         add_commute_function_schema,
         get_directions_function_schema,
         remove_commute_function_schema,
         start_live_directions_function_schema,
         create_agent_task_function_schema,
         create_countdown_function_schema,
     ]}
]
