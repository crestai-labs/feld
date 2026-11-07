from enum import StrEnum


class ObservationKind(StrEnum):
    GONE = "gone"
    LOST = "lost"
    MISMATCHED = "mismatched"
    RELOCATED = "relocated"
    RUNNING = "running"
    UNKNOWN = "unknown"

    def __str__(self) -> str:
        return str(self.value)
