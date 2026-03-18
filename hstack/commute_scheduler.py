"""
Commute Scheduler – runs as a background asyncio task.

Every 60 seconds it checks all COMMUTE-type tasks across all users.
If the current time is within 30 minutes before a commute deadline,
it calls the directions service every 5 minutes and stores an alert
that the frontend can poll via /api/commute-alerts/{userid}.
"""

import asyncio
import json
import time
from datetime import datetime, timedelta
from typing import TYPE_CHECKING, cast

from . import ai_tools
from . import database

if TYPE_CHECKING:
    from collections.abc import Callable, Awaitable

# Track when we last called directions per (userid, task_id) to enforce 5-min interval
_last_call: dict[tuple[int, str], float] = {}

# ── In-memory live trips (urgent one-time directions) ────────────────
# { trip_id: { "userid": int, "origin": str, "destination": str,
#              "deadline_ts": float, "created_ts": float, "label": str, "deadline_str": str, "task_id": str | None } }
_live_trips: dict[str, dict[str, object]] = {}
_live_trip_counter: int = 0

ALERT_WINDOW_MINUTES: int = 30   # start alerting this many minutes before deadline
CALL_INTERVAL_SECONDS: int = 300  # call directions every 5 minutes

def clear_user(userid: int) -> None:
    """
    Wipe all in-memory call timers for a user.
    
    Args:
        userid: The ID of the user to clear
    """
    # Remove all _last_call entries for this user
    keys = [k for k in _last_call if k[0] == userid]
    for k in keys:
        _last_call.pop(k, None)
    print(f"[CommuteScheduler] Cleared state for user {userid}")


def _is_in_alert_window(deadline_str: str, now: datetime) -> bool:
    """
    Check if `now` is within [deadline - 30min, deadline].
    
    Args:
        deadline_str: HH:MM deadline
        now: Current time
        
    Returns:
        True if within the 30-minute window
    """
    try:
        h, m = map(int, deadline_str.split(":"))
        deadline_today = now.replace(hour=h, minute=m, second=0, microsecond=0)
        window_start = deadline_today - timedelta(minutes=ALERT_WINDOW_MINUTES)
        return window_start <= now <= deadline_today
    except Exception:
        return False


def _day_matches(days_csv: str, now: datetime) -> bool:
    """
    Check if today's day name is in the comma-separated list.
    
    Args:
        days_csv: Comma-separated day names (e.g. "mon,wed,fri")
        now: Current time
        
    Returns:
        True if today matches the schedule
    """
    if not days_csv:
        # default: weekdays
        return now.weekday() < 5
    day_names = [d.strip().lower() for d in days_csv.split(",")]
    today = now.strftime("%A").lower()
    return today in day_names


_VEHICLE_ICON = {
    "BUS": "🚌",
    "SUBWAY": "🚇",
    "METRO_RAIL": "🚇",
    "RAIL": "🚆",
    "TRAM": "🚊",
    "COMMUTER_TRAIN": "🚆",
    "HIGH_SPEED_TRAIN": "🚄",
    "HEAVY_RAIL": "🚆",
    "LONG_DISTANCE_TRAIN": "🚆",
    "FERRY": "⛴️",
    "CABLE_CAR": "🚡",
    "FUNICULAR": "🚞",
    "SHARE_TAXI": "🚐",
    "TROLLEYBUS": "🚎",
}


def _format_commute_alert(label: str, routes: list[dict[str, object]], deadline_str: str, now: datetime) -> str:
    """
    Build a human-readable alert message from parsed routes.
    
    Args:
        label: Commute label
        routes: List of parsed route dictionaries
        deadline_str: HH:MM deadline
        now: Current time
        
    Returns:
        A multi-line formatted string for the user
    """
    if not routes:
        return f"No transit routes found for {label} right now."

    fastest = routes[0]  # already sorted by duration

    # Detect if the route has any transit steps
    steps = cast(list[dict[str, object]], fastest.get("steps", []))
    transit_steps = [s for s in steps if s.get("travel_mode") == "TRANSIT"]
    walk_only = len(transit_steps) == 0

    lines: list[str] = []
    lines.append(f"📍 {label.replace('_', ' ').title()}")

    # Duration header — only show dep/arr times when transit is involved
    dep = fastest.get('departure_time', '')
    arr = fastest.get('arrival_time', '')
    if dep and arr:
        lines.append(f"⏱ Total: {fastest['total_duration']}  (dep {dep} → arr {arr})")
    else:
        lines.append(f"⏱ Total: {fastest['total_duration']}")
    lines.append("")

    if walk_only:
        # Pure walking route
        lines.append(f"🚶 Walk the whole way – {fastest['total_duration']}")
        lines.append("No public transit needed for this distance.")
    else:
        # Build step-by-step itinerary
        for step in steps:
            mode = step.get("travel_mode", "")
            if mode == "WALKING":
                lines.append(f"🚶 Walk {step.get('duration', '')}")
            elif mode == "TRANSIT":
                vtype = str(step.get("vehicle_type", ""))
                icon = _VEHICLE_ICON.get(vtype, "🚍")
                line_name = step.get("transit_line", "?")
                dep_stop = step.get("departure_stop", "")
                arr_stop = step.get("arrival_stop", "")
                dep_time = step.get("departure_time", "")
                arr_time = step.get("arrival_time", "")
                num_stops = step.get("num_stops", 0)

                time_range = f"  {dep_time} → {arr_time}" if dep_time and arr_time else ""
                lines.append(f"{icon} {line_name}{time_range}  ({step.get('duration', '')})")
                if dep_stop:
                    lines.append(f"     Board at {dep_stop}")
                if arr_stop:
                    stop_label = f"{num_stops} stop{'s' if num_stops != 1 else ''}" if num_stops else ""
                    lines.append(f"     Exit at {arr_stop}" + (f"  ({stop_label})" if stop_label else ""))
                lines.append("")

    # Time until deadline
    if deadline_str:
        try:
            h, m = map(int, deadline_str.split(":"))
            deadline_dt = now.replace(hour=h, minute=m, second=0, microsecond=0)
            mins_to_deadline = max(0, int((deadline_dt - now).total_seconds() / 60))
            if mins_to_deadline <= 5:
                lines.append(f"🔴 {mins_to_deadline} min left before your {deadline_str} deadline!")
            else:
                lines.append(f"🎯 {mins_to_deadline} min left before your {deadline_str} deadline")
        except Exception:
            pass

    # Show alternative routes summary if available
    if len(routes) > 1:
        alts: list[str] = []
        for r in routes[1:3]:
            alt_transit: list[str] = []
            r_steps = cast(list[dict[str, object]], r.get("steps", []))
            for s in r_steps:
                if s.get("travel_mode") == "TRANSIT":
                    vt = str(s.get("vehicle_type", ""))
                    ic = _VEHICLE_ICON.get(vt, "🚍")
                    alt_transit.append(f"{ic}{s.get('transit_line', '?')}")
            chain = " → ".join(alt_transit) if alt_transit else "🚶 Walk"
            dep_t = r.get('departure_time', '')
            dep_info = f", dep {dep_t}" if dep_t else ""
            alts.append(f"  {chain}  ({r['total_duration']}{dep_info})")
        lines.append("")
        lines.append("Other options:")
        lines.extend(alts)

    return "\n".join(lines)


# ── Live trip management ─────────────────────────────────────────────

def register_live_trip(userid: int, origin: str, destination: str,
                       minutes_until_deadline: int, task_id: str | None = None) -> str:
    """
    Register an urgent one-time trip.
    
    Args:
        userid: Owner ID
        origin: Start address
        destination: End address
        minutes_until_deadline: Time left
        task_id: UUID of the persistent ticket
        
    Returns:
        Local trip ID
    """
    global _live_trip_counter
    _live_trip_counter += 1
    trip_id = f"live_{_live_trip_counter}"
    now_f = time.time()
    deadline_ts = now_f + (minutes_until_deadline * 60)
    deadline_dt = datetime.fromtimestamp(deadline_ts)

    _live_trips[trip_id] = {
        "userid": userid,
        "origin": origin,
        "destination": destination,
        "deadline_ts": deadline_ts,
        "created_ts": now_f,
        "label": f"Trip to {destination[:40]}",
        "deadline_str": deadline_dt.strftime("%H:%M"),
        "task_id": task_id
    }
    # Force immediate first call by NOT setting _last_call
    print(f"[LiveTrip] Registered {trip_id} for user {userid}")
    return trip_id


def get_active_live_trips(userid: int) -> list[dict[str, object]]:
    """
    Return all active live trips for a user.
    
    Args:
        userid: The user's ID
        
    Returns:
        List of active trip info
    """
    now_f = time.time()
    return [
        {**trip, "trip_id": tid,
         "minutes_remaining": max(0, int((cast(float, trip["deadline_ts"]) - now_f) / 60))}
        for tid, trip in _live_trips.items()
        if trip["userid"] == userid and cast(float, trip["deadline_ts"]) > now_f
    ]


async def _check_live_trips(broadcast_callback: "Callable[[int], Awaitable[None]] | None" = None) -> None:
    """
    Check all live trips, call directions every 5 min, expire when deadline passes.
    
    Args:
        broadcast_callback: Optional callback to notify frontend of updates.
    """
    now_ts = time.time()
    expired: list[str] = []

    for trip_id, trip in _live_trips.items():
        userid = cast(int, trip["userid"])
        deadline_ts = cast(float, trip["deadline_ts"])
        task_id = cast(str | None, trip.get("task_id"))
        
        # Expired?
        if now_ts > deadline_ts:
            expired.append(trip_id)
            if task_id:
                try:
                    _ = await database.update_task_status(task_id, "expired")
                    if broadcast_callback:
                        await broadcast_callback(userid)
                except Exception as e:
                    print(f"[LiveTrip] Failed to set expired status: {e}")
            continue

        # Enforce 5-min interval
        key = (userid, trip_id)
        last = _last_call.get(key, 0.0)
        if now_ts - last < CALL_INTERVAL_SECONDS:
            continue

        _last_call[key] = now_ts
        mins_left = max(0, int((deadline_ts - now_ts) / 60))

        try:
            origin = str(trip["origin"])
            destination = str(trip["destination"])
            raw_response = await ai_tools.call_directions_service(origin, destination)
            raw_routes = cast(list[dict[str, object]], raw_response.get("routes", []))
            parsed = ai_tools.parse_transit_directions(raw_routes)
            
            # Update ticket payload
            if task_id:
                existing = await database.get_task(task_id)
                if existing:
                    payload_raw = existing.get("payload")
                    curr_payload: dict[str, object] = {}
                    if isinstance(payload_raw, str):
                        curr_payload = cast(dict[str, object], json.loads(payload_raw))
                    
                    curr_payload["directions"] = {
                        "steps": parsed[0].get("steps", []) if parsed else [],
                        "total_duration": parsed[0].get("total_duration", "Unknown") if parsed else "Unknown",
                        "departure_time": parsed[0].get("departure_time", "") if parsed else "",
                        "arrival_time": parsed[0].get("arrival_time", "") if parsed else "",
                        "error": None if parsed else "No routes found"
                    }
                    curr_payload["minutes_remaining"] = mins_left
                    _ = await database.update_task_payload(task_id, json.dumps(curr_payload))
                
                if broadcast_callback:
                    await broadcast_callback(userid)
            
        except Exception as e:
            print(f"[LiveTrip] Directions failed: {e}")
            if task_id:
                try:
                    existing = await database.get_task(task_id)
                    if existing:
                        p_raw = existing.get("payload")
                        curr_p: dict[str, object] = {}
                        if isinstance(p_raw, str):
                            curr_p = cast(dict[str, object], json.loads(p_raw))
                        
                        directions = cast(dict[str, object], curr_p.get("directions", {}))
                        directions["error"] = str(e)
                        curr_p["directions"] = directions
                        curr_p["minutes_remaining"] = mins_left
                        _ = await database.update_task_payload(task_id, json.dumps(curr_p))
                        if broadcast_callback:
                            await broadcast_callback(userid)
                except Exception: pass

    # Clean up expired trips
    for tid in expired:
        _live_trips.pop(tid, None)
        keys_to_del = [k for k in _last_call if k[1] == tid]
        for k in keys_to_del:
            _last_call.pop(k, None)


async def _check_commutes(broadcast_callback: "Callable[[int], Awaitable[None]] | None" = None) -> None:
    """
    Single pass: check all commute tasks and update payloads directly.
    
    Args:
        broadcast_callback: Optional callback to notify frontend of updates.
    """
    try:
        if database.pool is None:
            return

        # Fetch all COMMUTE tasks across all users
        async with database.pool.acquire() as conn:
            rows = await conn.fetch(
                "SELECT * FROM public.task WHERE type = 'COMMUTE'"
            )

        now = datetime.now()

        for row in rows:
            record: dict[str, object] = dict(row)
            userid = cast(int | None, record.get("userid"))
            task_id = str(record.get("id"))
            payload_raw = record.get("payload")
            payload: dict[str, object] = {}
            if isinstance(payload_raw, str):
                try:
                    payload = cast(dict[str, object], json.loads(payload_raw))
                except Exception:
                    continue
            
            if not payload or payload.get("live"):
                continue

            label = str(payload.get("label", "commute"))
            origin = cast(str | None, payload.get("origin"))
            destination = cast(str | None, payload.get("destination"))
            deadline = cast(str | None, payload.get("deadline"))
            days = str(payload.get("days", ""))

            if not all([origin, destination, deadline]):
                continue

            if not _day_matches(days, now):
                continue
            if not deadline or not _is_in_alert_window(deadline, now):
                continue

            if userid is None:
                continue
            
            # Enforce 5-minute interval per commute
            key: tuple[int, str] = (userid, task_id)
            last = _last_call.get(key, 0.0)
            if time.time() - last < CALL_INTERVAL_SECONDS:
                continue

            _last_call[key] = time.time()

            # Call directions service
            try:
                if origin and destination:
                    raw_res = await ai_tools.call_directions_service(origin, destination)
                    routes_raw = cast(list[dict[str, object]], raw_res.get("routes", []))
                    parsed = ai_tools.parse_transit_directions(routes_raw)
                    
                    payload["directions"] = {
                        "steps": parsed[0].get("steps", []) if parsed else [],
                        "total_duration": parsed[0].get("total_duration", "Unknown") if parsed else "Unknown",
                        "departure_time": parsed[0].get("departure_time", "") if parsed else "",
                        "arrival_time": parsed[0].get("arrival_time", "") if parsed else ""
                    }
                    
                    _ = await database.update_task(task_id, payload=json.dumps(payload), status="in_focus")
                    
                    if broadcast_callback:
                        await broadcast_callback(userid)
            except Exception as e:
                print(f"[CommuteScheduler] Call failed: {e}")

    except Exception as e:
        print(f"[CommuteScheduler] Error in loop: {e}")


async def run_scheduler(broadcast_callback: "Callable[[int], Awaitable[None]] | None" = None) -> None:
    """
    Background loop – runs forever, checking every 60 seconds.
    
    Args:
        broadcast_callback: Optional callback to notify frontend of updates.
    """
    print("[CommuteScheduler] Started ✓")
    while True:
        await _check_commutes(broadcast_callback)
        await _check_live_trips(broadcast_callback)
        await asyncio.sleep(60)
