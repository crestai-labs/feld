from enum import StrEnum


class SessionEndKind(StrEnum):
    EXITED = "exited"
    RUNNING = "running"
    STOPPED = "stopped"
    UNKNOWN = "unknown"

    def __str__(self) -> str:
        return str(self.value)
