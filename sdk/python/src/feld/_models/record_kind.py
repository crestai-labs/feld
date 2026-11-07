from enum import StrEnum


class RecordKind(StrEnum):
    KILL_REQUESTED = "kill.requested"
    LABEL_CLEARED = "label.cleared"
    LABEL_EXPIRED = "label.expired"
    LABEL_SET = "label.set"
    MESSAGE_ACCEPTED = "message.accepted"
    MESSAGE_READ = "message.read"
    MESSAGE_SENT = "message.sent"
    OBSERVATION = "observation"
    SCHEDULE_MISSED = "schedule.missed"
    SCHEDULE_PAUSED = "schedule.paused"
    SCHEDULE_RESUMED = "schedule.resumed"
    SCHEDULE_SKIPPED = "schedule.skipped"
    SESSION_ENDED = "session.ended"
    SESSION_STARTED = "session.started"
    SPAWNED = "spawned"
    STOP_REQUESTED = "stop.requested"

    def __str__(self) -> str:
        return str(self.value)
