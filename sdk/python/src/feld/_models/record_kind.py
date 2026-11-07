from enum import StrEnum


class RecordKind(StrEnum):
    KILL_REQUESTED = "kill.requested"
    MESSAGE_ACCEPTED = "message.accepted"
    MESSAGE_READ = "message.read"
    MESSAGE_SENT = "message.sent"
    OBSERVATION = "observation"
    SCHEDULE_MISSED = "schedule.missed"
    SCHEDULE_SKIPPED = "schedule.skipped"
    SESSION_ENDED = "session.ended"
    SESSION_STARTED = "session.started"
    SPAWNED = "spawned"
    STOP_REQUESTED = "stop.requested"

    def __str__(self) -> str:
        return str(self.value)
