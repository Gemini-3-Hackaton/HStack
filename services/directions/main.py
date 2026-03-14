import os
import googlemaps
from datetime import datetime
from fastapi import FastAPI, HTTPException
from pydantic import BaseModel

app = FastAPI(title="Directions API")

API_KEY = os.environ.get("GOOGLE_MAPS_API_KEY", "")


class DirectionRequest(BaseModel):
    origin: str
    destination: str


@app.post("/directions")
def get_directions(body: DirectionRequest):
    if not API_KEY:
        raise HTTPException(status_code=500, detail="GOOGLE_MAPS_API_KEY is not set")

    gmaps = googlemaps.Client(key=API_KEY)

    directions = gmaps.directions(
        origin=body.origin,
        destination=body.destination,
        mode="transit",
        alternatives=True,
        departure_time=datetime.now()  # Or arrival_time
    )

    return directions
