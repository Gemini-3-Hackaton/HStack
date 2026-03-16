import unittest
from unittest.mock import patch

from fastapi.testclient import TestClient
from googlemaps.exceptions import ApiError

import services.directions.main as directions_main


class DirectionsServiceTests(unittest.TestCase):
    client: TestClient | None = None
    original_api_key: str | None = None

    def setUp(self):
        self.client = TestClient(directions_main.app)
        self.original_api_key = directions_main.os.environ.get("GOOGLE_MAPS_API_KEY")
        directions_main.os.environ["GOOGLE_MAPS_API_KEY"] = "test-key"

    def tearDown(self):
        if self.original_api_key is None:
            directions_main.os.environ.pop("GOOGLE_MAPS_API_KEY", None)
        else:
            directions_main.os.environ["GOOGLE_MAPS_API_KEY"] = self.original_api_key

    def test_rejects_blank_origin_or_destination(self):
        if self.client is None: self.fail("Client not initialized")
        response = self.client.post(
            "/directions",
            json={"origin": "   ", "destination": "Paris"},
        )

        self.assertEqual(response.status_code, 400)
        self.assertEqual(
            response.json()["detail"],
            "Both origin and destination are required.",
        )

    @patch.object(directions_main.googlemaps, "Client")
    def test_maps_denial_returns_actionable_error(self, client_cls):
        client_cls.return_value.directions.side_effect = ApiError(
            "REQUEST_DENIED",
            "The provided API key is invalid.",
        )

        if self.client is None: self.fail("Client not initialized")
        response = self.client.post(
            "/directions",
            json={"origin": "Paris", "destination": "Lyon"},
        )

        self.assertEqual(response.status_code, 502)
        self.assertEqual(
            response.json()["detail"],
            "Google Maps request denied. Check GOOGLE_MAPS_API_KEY, API restrictions, and billing.",
        )

    @patch.object(directions_main.googlemaps, "Client")
    def test_returns_routes_on_success(self, client_cls):
        expected = [{"legs": [{"duration": {"text": "12 min", "value": 720}, "steps": []}]}]
        client_cls.return_value.directions.return_value = expected

        if self.client is None: self.fail("Client not initialized")
        response = self.client.post(
            "/directions",
            json={"origin": "Paris", "destination": "Lyon"},
        )

        self.assertEqual(response.status_code, 200)
        self.assertEqual(response.json(), expected)


if __name__ == "__main__":
    unittest.main()
