from enum import StrEnum


class SessionOwnership(StrEnum):
    ATTACHED = "attached"
    OWNED = "owned"

    def __str__(self) -> str:
        return str(self.value)
