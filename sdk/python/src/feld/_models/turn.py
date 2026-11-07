from enum import StrEnum


class Turn(StrEnum):
    RESPONDING = "responding"
    UNAVAILABLE = "unavailable"
    WAITING_INPUT = "waiting-input"

    def __str__(self) -> str:
        return str(self.value)
