from enum import StrEnum


class Outcome(StrEnum):
    ENDED = "ended"
    EXITED = "exited"
    FAILED = "failed"
    RUNNING = "running"
    STOPPED = "stopped"
    SUCCEEDED = "succeeded"
    UNKNOWN = "unknown"

    def __str__(self) -> str:
        return str(self.value)
