# Unified Ticket Philosophy

## Core Principle

**Everything is a Ticket.** There is one visual primitive, one data model, one component. No alerts, no toasts, no special-cased UI. A ticket is the atomic unit of HStack.

---

## What is a Ticket?

A ticket is a **persistent, stateful object** in the user's stack. It has:

| Field | Type | Purpose |
|-------|------|---------|
| `id` | UUID | Unique identifier |
| `type` | enum | Drives grain theme + behavior |
| `status` | enum | Lifecycle state |
| `payload` | dict | Rich, type-specific content |
| `created_at` | datetime | Creation timestamp |
| `updated_at` | datetime | Last modification |

### Types

| Type | Theme | Description |
|------|-------|-------------|
| `TASK` | Default gray | Generic to-do |
| `HABIT` | Emerald tint | Recurring behavior |
| `EVENT` | Amber tint | Calendar-bound occurrence |
| `COMMUTE` | Purple tint | Transit/directions information |
| `COUNTDOWN` | Blue tint | Time-bound deadline |

> [!IMPORTANT]
> `AGENT_TASK` is removed. Agent work is just a `COUNTDOWN` with context in the payload.

### Statuses

| Status | Meaning |
|--------|---------|
| `idle` | Default state. Rendered normally. |
| `in_focus` | Expanded, prominent. Replaces the concept of "alerts". |
| `completed` | Dimmed, struck through. |
| `expired` | Timer ran out. Visually distinct, archivable. |

---

## The "In Focus" Concept

**In Focus replaces alerts entirely.** When the AI returns directions, it doesn't push an ephemeral toast — it creates a `COMMUTE` ticket with `status: in_focus`.

### Rules

1. Only **one ticket** can be `in_focus` at a time
2. An `in_focus` ticket renders **expanded** — showing rich content (directions, map data, countdown)
3. The user can **dismiss** focus (ticket goes back to `idle`) or **complete** it
4. Focus is a **status**, not a component — the same `TicketCard` handles everything

### Visual Behavior

- `idle` → Compact card (title + tags)
- `in_focus` → Expanded card, same moat wrapper but with visible rich content block
- `completed` → Dimmed (`opacity-50`), strike-through title  
- `expired` → Subtle visual cue (desaturated grain, muted text)

---

## The COMMUTE Ticket (Design Spec)

A commute ticket is the richest ticket type. Its payload can contain:

```
payload: {
  title: "Home → Office"
  origin: "123 Rue Example, Paris"
  destination: "456 Avenue Work, Paris"
  directions: {
    total_duration: "34 min"
    departure_time: "8:26 AM"
    arrival_time: "9:00 AM"
    steps: [
      { mode: "WALKING", duration: "5 min", instruction: "Walk to Métro..." }
      { mode: "TRANSIT", duration: "22 min", line: "M6", from: "Pasteur", to: "Étoile" }
      { mode: "WALKING", duration: "7 min", instruction: "Walk to destination" }
    ]
  }
  live: true | false
  expires_at: "2026-03-18T09:00:00Z"   // For live trips
  recurrence: "WEEKDAYS"                // For registered commutes
}
```

### Rendering When In Focus

The expanded commute ticket shows:

1. **Route summary** — "34 min • Depart 8:26 AM"
2. **Step-by-step** — Each transit step as a mini-row (icon + line name + stops + duration)
3. **Live indicator** — Pulsing dot if `live: true`, with countdown to deadline
4. **Refresh hint** — "Updated 2 min ago" for live trips

### Rendering When Idle

Collapsed to a single line: **"Home → Office"** with a `COMMUTE` tag and time.

---

## Data Flow Changes

### Current (Broken)

```
User asks for directions
  → AI returns { action: "get_directions", response: "text blob" }
  → Frontend pushes to local `alerts[]` state
  → Rendered as ephemeral CommuteAlert component
  → Lost on refresh
```

### Proposed (Unified)

```
User asks for directions
  → AI calls get_directions tool
  → Backend creates a COMMUTE ticket with parsed directions in payload
  → Backend sets status: "in_focus"
  → SyncProvider pushes ticket to frontend
  → TicketCard renders it expanded (same component as everything else)
  → Persisted in DB, survives refresh
```

> [!NOTE]
> The `get_directions` and `start_live_directions` tools now create tickets instead of returning text responses. The frontend no longer needs a separate `alerts` state array.

---

## Frontend Impact

### Remove
- `CommuteAlert` component
- `alerts` state array in `App.tsx`
- All alert-specific rendering logic

### Modify
- `TicketCard` gains an `in_focus` expanded state
- `TicketCard` gains rich content slots (directions steps, countdown timer, etc.)
- `groupTasks()` — `in_focus` tickets render at the top, before scope groups

### Add
- `TicketExpanded` sub-component (rendered inside TicketCard when `status === 'in_focus'`)
- `CommuteSteps` sub-component (step-by-step transit rendering)
- `status` field handling in `SyncEngine.tsx`

---

## Backend Impact

### Models
- Add `status` field to `TaskModel` (default: `idle`)
- Remove `AGENT_TASK` from `TicketType` (use `COUNTDOWN` instead)

### AI Tools
- `get_directions` → creates COMMUTE ticket with parsed directions payload, status `in_focus`
- `start_live_directions` → creates COMMUTE ticket with `live: true`, same approach
- `create_agent_task` → creates COUNTDOWN ticket instead
- Remove ephemeral text responses for directions

### API
- Add endpoint or sync action for setting ticket status (`set_focus`, `dismiss`, `complete`)

---

## Design Principle Summary

| Old | New |
|-----|-----|
| Two components (TicketCard + CommuteAlert) | One component (TicketCard) |
| Ephemeral alerts (lost on refresh) | Persistent tickets (synced) |
| Special rendering per type | Unified rendering with expandable content |
| `alerts[]` local state | Everything in `tasks[]` via SyncProvider |
| Type determines component | Type determines theme; status determines layout |
