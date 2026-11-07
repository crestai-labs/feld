from enum import StrEnum


class ConsoleState(StrEnum):
    LOST = "lost"
    MISSING = "missing"
    PENDING = "pending"
    READABLE = "readable"

    def __str__(self) -> str:
        return str(self.value)
