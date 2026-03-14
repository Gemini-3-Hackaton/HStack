
# HStack
The auto-kanban for normal people that want to have to think about less things.

## Starting the Server

This project uses `uv` for lightning-fast Python dependency management and runs a FastAPI backend with a vanilla HTML/JS/CSS frontend.

To run the application locally:

1. **Ensure you are in the project directory:**
   ```bash
   cd /home/antoine/Documents/perso/HStack
   ```

2. **Start the development server:**
   ```bash
   uv run uvicorn main:app --port 8080 --reload
   ```

3. **Open the App:**
   Open your browser and navigate to [http://127.0.0.1:8080](http://127.0.0.1:8080) to view the Linear-inspired interface.

> **Note:** The server defaults to port `8080`. If you encounter an `Address already in use` error, you might have another instance of the server already running in the background. You can stop previous instances or run it on a different port like `--port 8081`.
