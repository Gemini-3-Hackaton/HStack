from typing import TYPE_CHECKING, cast
if TYPE_CHECKING:
    pass
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
async def call_directions_service(origin: str, destination: str) -> dict[str, object]:
    """
    Call the directions microservice and return parsed transit info.
    
    Args:
        origin: Starting address
        destination: Destination address
        
    Returns:
        JSON response from the directions service
    """
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
        except (httpx.ConnectError, httpx.HTTPError, httpx.TimeoutException) as exc:
            print(f"Directions service error: {exc}")
            raise RuntimeError(f"Directions service at {DIRECTIONS_SERVICE_URL} is currently unreachable.") from exc

    if resp.is_error:
        detail: str | None = None
        try:
            payload: dict[str, object] = resp.json()
        except ValueError:
            payload = {}
            
        if isinstance(payload, dict):
            detail = str(payload.get("detail", ""))
        raise RuntimeError(detail or f"Directions service returned HTTP {resp.status_code}.")

    return resp.json()


def parse_transit_directions(raw_routes: list[dict[str, object]]) -> list[dict[str, object]]:
    """
    Parse raw Google Maps directions response into a clean list of route summaries.
    
    Args:
        raw_routes: List of raw route dictionaries from Google Maps API
        
    Returns:
        List of parsed route summaries
    """
    parsed: list[dict[str, object]] = []
    for route in raw_routes:
        legs = cast(list[dict[str, object]], route.get("legs", [{}]))
        leg = legs[0] if legs else {}
        
        steps_info: list[dict[str, object]] = []
        steps = cast(list[dict[str, object]], leg.get("steps", []))
        
        for step in steps:
            travel_mode = str(step.get("travel_mode", ""))
            duration_dict = cast(dict[str, object], step.get("duration", {}))
            info: dict[str, object] = {
                "travel_mode": travel_mode,
                "duration": duration_dict.get("text", ""),
                "instruction": step.get("html_instructions", ""),
            }
            transit = cast(dict[str, object], step.get("transit_details", {}))
            if transit:
                line = cast(dict[str, object], transit.get("line", {}))
                info["transit_line"] = line.get("short_name") or line.get("name", "")
                vehicle = cast(dict[str, object], line.get("vehicle", {}))
                info["vehicle_type"] = vehicle.get("type", "")
                dep_stop = cast(dict[str, object], transit.get("departure_stop", {}))
                arr_stop = cast(dict[str, object], transit.get("arrival_stop", {}))
                info["departure_stop"] = dep_stop.get("name", "")
                info["arrival_stop"] = arr_stop.get("name", "")
                dep_time = cast(dict[str, object], transit.get("departure_time", {}))
                arr_time = cast(dict[str, object], transit.get("arrival_time", {}))
                info["departure_time"] = dep_time.get("text", "")
                info["departure_timestamp"] = dep_time.get("value")
                info["arrival_time"] = arr_time.get("text", "")
                info["num_stops"] = transit.get("num_stops", 0)
            steps_info.append(info)

        leg_dur = cast(dict[str, object], leg.get("duration", {}))
        leg_dep = cast(dict[str, object], leg.get("departure_time", {}))
        leg_arr = cast(dict[str, object], leg.get("arrival_time", {}))
        parsed.append({
            "summary": route.get("summary", ""),
            "total_duration": leg_dur.get("text", ""),
            "total_duration_sec": leg_dur.get("value", 0),
            "departure_time": leg_dep.get("text", ""),
            "arrival_time": leg_arr.get("text", ""),
            "steps": steps_info,
        })
    # Sort by total duration
    def _sort_key(r: dict[str, object]) -> int:
        val = r.get("total_duration_sec", 0)
        return int(val) if isinstance(val, (int, float, str)) else 0
    parsed.sort(key=_sort_key)
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
            },
            "scheduled_time": {
                "type": "STRING",
                "description": "Optional: Specific time for the ticket (e.g., '9 AM', '14:30', '2026-03-17 15:00'). Triggers the 'Scope' visual sidebar."
            },
            "duration_minutes": {
                "type": "INTEGER",
                "description": "Optional: Estimated duration in minutes."
            },
            "recurrence": {
                "type": "STRING",
                "description": "Optional: Recurrence DSL for HABITs. Use formats like: 'DAILY', 'WEEKDAYS', 'MON, WED, FRI', '9TH OF MONTH', '9TH, 10TH OF MONTH', '1ST MON OF MONTH'."
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
    "description": "Edit an existing ticket in the user's stack. You can change its type, title, or timing.",
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
            },
            "scheduled_time": {
                "type": "STRING",
                "description": "The new scheduled time. Skip if no change."
            },
            "duration_minutes": {
                "type": "INTEGER",
                "description": "The new duration. Skip if no change."
            },
            "recurrence": {
                "type": "STRING",
                "description": "The new recurrence pattern. Skip if no change."
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
    "description": "Get real-time transit directions between two places. This creates a persistent COMMUTE ticket in the user's stack that renders expanded (in-focus) with step-by-step instructions.",
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
    "description": """Start a live directions tracker for an URGENT or ONE-TIME trip with a deadline.
This creates a persistent COMMUTE ticket with `live: true` that stays in-focus and updates every 5 minutes until the deadline passes.""",
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

create_countdown_function_schema = {
    "name": "create_countdown",
    "description": """Create a countdown timer (personal or agent-related). Use this for any task with a time limit (e.g., 'eat in 30 mins', 'IDE refactoring for 10 mins').
This creates a COUNTDOWN ticket with a live timer that auto-deletes when it expires.""",
    "parameters": {
        "type": "OBJECT",
        "properties": {
            "title": {
                "type": "STRING",
                "description": "A short description of the task or timer, e.g., 'Refactoring code' or 'Time to leave'"
            },
            "duration_minutes": {
                "type": "INTEGER",
                "description": "Number of minutes until the deadline."
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
         create_countdown_function_schema,
     ]}
]
