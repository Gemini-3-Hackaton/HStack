
# HStack : First step toward symbiosis


**The auto-kanban for normal people who want to think about less things.**

HStack is an AI-native task management system powered by Google Gemini. You talk to it in plain language — it organizes your life into a visual stack of tickets, tracks your commutes in real time, and monitors your background agent work with countdown timers.

No forms. No dropdowns. Just tell it what's going on.

![alt text](docs/assets/image.png)
---

## Core Concept

HStack replaces traditional task boards with a single conversational interface. Every action — creating tasks, scheduling commutes, checking directions — flows through a natural language chat backed by Gemini function calling. The AI decomposes complex requests into discrete tool calls automatically.

---

## Features

### Ticket Management
Talk to the AI to create, edit, delete, and complete tickets. Each ticket is auto-classified:

| Type | Purpose | Example |
|------|---------|---------|
| **TASK** | One-off action items | *"Buy groceries"* |
| **HABIT** | Daily routines and recurring behaviors | *"Exercise every morning"* |
| **EVENT** | Time-specific appointments | *"Dentist at 3pm tomorrow"* |
| **COMMUTE** | Recurring transit routes with live alerts | *"I go from Asnières to Saint-Lazare every morning at 9:30"* |
| **AGENT_TASK** | Background AI/IDE work with countdown timer | *"VSCode is refactoring my auth module"* |

The AI handles multi-action decomposition — a message like *"Get laundry detergent for mum and kibble for the cat"* becomes multiple discrete tickets in one turn.

### Commute Management
Describe a recurring trip and HStack registers it as a commute:

- **Automatic alerts** — 30 minutes before your deadline, the system starts polling Google Maps Directions every 5 minutes
- **Transit-first** — shows departure times, line names, and walk segments for public transit routes
- **Day scheduling** — specify which days (defaults to weekdays) and arrival time in HH:MM
- **Background scheduler** — an asyncio loop checks all commutes every 60 seconds, no user action needed

### Live / Urgent Directions
For one-time urgent trips (*"I need to get to the airport in 45 minutes"*):

- Immediate directions response with transit options
- Automatic re-polling every 5 minutes until the deadline expires
- Persistent banner notifications with a pulsing **LIVE** badge
- Auto-cleanup when the deadline passes

### Agent Task Timers
When an AI agent or IDE is working on something in the background:

- Creates a ticket with a live **MM:SS countdown timer**
- Default duration: 10 minutes (customizable)
- Auto-deletes from the database when the timer expires
- Visual pulsing amber indicator on the ticket card
- Triggered by natural phrases: *"Cursor is fixing the tests"*, *"Copilot is generating the migration"*

### Notification System
- **Single notification policy** — only one alert banner visible at a time; new alerts replace the previous one
- **Persistent banners** — active commute/live-trip alerts stay on screen until replaced by the next update
- **Expired alerts** auto-dismiss after 10 seconds
- **Reset clears everything** — clearing the stack also cancels all schedulers, live trips, and alert banners

---

## Architecture

```
┌─────────────────────────────────────────────────┐
│                  Frontend (Tauri)                │
│       React · WebGL Shaders · Tailwind CSS       │
│    Chat → /api/chat    Polls → /api/sync         │
└───────────────┬─────────────────┬───────────────┘
                │                 │
        ┌───────▼───────┐  ┌─────▼──────────────┐
        │  FastAPI :8080 │  │ Commute Scheduler  │
        │   main.py      │  │ (asyncio background)│
        │   Gemini AI    │  │ commute_scheduler.py│
        └───────┬───────┘  └─────┬──────────────┘
                │                 │
        ┌───────▼─────────────────▼───────┐
        │       Directions Service :8001   │
        │   Google Maps Directions API     │
        │   services/directions/main.py    │
        └─────────────────────────────────┘
                │
        ┌───────▼───────┐
        │  PostgreSQL    │
        │  (Supabase)    │
        └───────────────┘
```

| Component | Tech | Role |
|-----------|------|------|
| **Backend** | FastAPI, Python 3.11+ | API server, AI orchestration, tool dispatch |
| **AI** | Google Gemini (`gemini-flash-latest`) | Natural language → function calls |
| **Directions** | Google Maps API via `googlemaps` | Transit routing microservice |
| **Database** | PostgreSQL (Supabase) via `asyncpg` | Persistent task/user storage |
| **Scheduler** | asyncio background task | Periodic commute checks, live trip polling |
| **Frontend** | Vanilla JS, WebGL, CSS | Chat UI, ticket cards, alert banners |

---

## Getting Started

### Prerequisites
- Python 3.11+
- [`uv`](https://docs.astral.sh/uv/) package manager
- A PostgreSQL database (Supabase recommended)
- Google Gemini API key
- Google Maps API key (with Directions API enabled)

### Environment Variables

Create a `.env` file at the project root:

```env
DATABASE_URL=postgresql://...
GEMINI_API_KEY=your_gemini_key
GOOGLE_MAPS_API_KEY=your_google_maps_key
DIRECTIONS_SERVICE_URL=http://localhost:8001   # optional, this is the default
```

### Running

**1. Start the Backend Server:**

```bash
npx varlock run -- uv run uvicorn hstack.main:app --port 8000
```

**2. Start the Tauri Desktop App:**

```bash
cd frontend
npm install
npm run tauri dev
```

The app will launch in a standalone, borderless window.

---

## API Endpoints

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/` | Serve the frontend |
| `GET` | `/api/tasks?userid=N` | Fetch all tickets for a user |
| `POST` | `/api/chat` | Send a message to Gemini (creates/edits/deletes tickets) |
| `POST` | `/api/auth/register` | Create a new user account |
| `POST` | `/api/auth/login` | Authenticate an existing user |
| `GET` | `/api/commute-alerts/{userid}` | Poll for pending commute/live-trip alerts |
| `GET` | `/api/commutes/{userid}` | List registered recurring commutes |
| `GET` | `/api/live-trips/{userid}` | List active live/urgent trips |

---

## AI Tool System

Gemini has access to these function-calling tools:

| Tool | Action |
|------|--------|
| `create_ticket` | Create a TASK, HABIT, or EVENT |
| `edit_ticket` | Modify an existing ticket's title or type |
| `delete_ticket` | Remove a specific ticket by ID |
| `delete_all_tickets` | Clear the entire stack |
| `add_commute` | Register a recurring commute with schedule |
| `remove_commute` | Delete a registered commute |
| `get_directions` | One-shot transit directions between two points |
| `start_live_directions` | Begin live tracking for an urgent trip |
| `create_agent_task` | Start a timed background agent task |

The AI can invoke **multiple tools in a single turn** to decompose complex requests.

---

## Project Structure

```
HStack/
├── main.py                  # FastAPI app, chat endpoint, all tool handlers
├── ai_tools.py              # Gemini client, tool schemas, directions helpers
├── commute_scheduler.py     # Background scheduler for commutes & live trips
├── models.py                # Pydantic models, TicketType enum
├── database.py              # asyncpg database operations
├── pyproject.toml           # Project config & dependencies
├── requirements.txt         # Pip-compatible dependencies
├── services/
│   └── directions/
│       └── main.py          # Google Maps Directions microservice
└── static/
    ├── index.html           # Frontend HTML
    ├── app.js               # Frontend logic, polling, alerts
    └── style.css            # Dark theme, ticket styles, animations
```

---

## Vision

HStack is building toward a world where your task board is a living, breathing system that understands context, not just input.

**Where we are:**
- Natural language ticket management with AI decomposition
- Real-time transit awareness baked into daily planning
- Background agent monitoring as a first-class task type

**Where we're going:**
- **Contextual auto-scheduling** — the system learns your patterns and pre-populates your day before you wake up
- **Cross-agent orchestration** — HStack becomes the central hub that dispatches work to Cursor, Copilot, Claude, and other agents, tracking all of them with live timers
- **Predictive commute intelligence** — instead of polling on a schedule, the system anticipates delays and proactively reroutes you
- **Ambient awareness** — calendar, weather, traffic, and energy levels feed into ticket prioritization automatically
- **Multi-user coordination** — shared stacks where teams see each other's context without status meetings
- **Voice-first interface** — talk to HStack while walking, driving, or cooking — no screen required

The end state: a system that thinks about your day so you don't have to.
