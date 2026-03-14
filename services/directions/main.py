import os
from datetime import datetime
from pathlib import Path

import dotenv
import googlemaps
from fastapi import FastAPI, HTTPException
from googlemaps import exceptions as gmaps_exceptions
from pydantic import BaseModel, ConfigDict

SERVICE_DIR = Path(__file__).resolve().parent
dotenv.load_dotenv(SERVICE_DIR.parent.parent / ".env", override=False)
dotenv.load_dotenv(SERVICE_DIR / ".env", override=True)

app = FastAPI(title="Directions API")


def _get_api_key() -> str:
    key = os.environ.get("GOOGLE_MAPS_API_KEY", "").strip()
    if not key:
        # Fallback: try re-loading .env in case it wasn't picked up at module level
        dotenv.load_dotenv(SERVICE_DIR.parent.parent / ".env", override=True)
        dotenv.load_dotenv(SERVICE_DIR / ".env", override=True)
        key = os.environ.get("GOOGLE_MAPS_API_KEY", "").strip()
    return key


def _map_google_error_status(status: str) -> int:
    if status in {"INVALID_REQUEST", "NOT_FOUND"}:
        return 400
    if status in {"OVER_DAILY_LIMIT", "OVER_QUERY_LIMIT"}:
        return 429
    return 502


def _format_google_error(exc: gmaps_exceptions.ApiError) -> str:
    if exc.status == "REQUEST_DENIED":
        return "Google Maps request denied. Check GOOGLE_MAPS_API_KEY, API restrictions, and billing."
    if exc.message:
        return f"Google Maps error: {exc.message}"
    return f"Google Maps error: {exc.status}"


class DirectionRequest(BaseModel):
    model_config = ConfigDict(str_strip_whitespace=True)

    origin: str
    destination: str


@app.post("/directions")
def get_directions(body: DirectionRequest):
    api_key = _get_api_key()
    if not api_key:
        raise HTTPException(status_code=500, detail="GOOGLE_MAPS_API_KEY is not set")
    if not body.origin or not body.destination:
        raise HTTPException(status_code=400, detail="Both origin and destination are required.")

    gmaps = googlemaps.Client(key=api_key)

    try:
        return gmaps.directions(
            origin=body.origin,
            destination=body.destination,
            mode="transit",
            alternatives=True,
            departure_time=datetime.now(),  # Or arrival_time
        )
    except gmaps_exceptions.ApiError as exc:
        raise HTTPException(
            status_code=_map_google_error_status(exc.status),
            detail=_format_google_error(exc),
        ) from exc
    except gmaps_exceptions.Timeout as exc:
        raise HTTPException(status_code=504, detail="Google Maps request timed out.") from exc
    except gmaps_exceptions.TransportError as exc:
        raise HTTPException(status_code=502, detail=f"Google Maps transport error: {exc}") from exc
