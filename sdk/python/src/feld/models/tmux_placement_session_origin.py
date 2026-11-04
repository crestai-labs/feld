from enum import StrEnum


class TmuxPlacementSessionOrigin(StrEnum):
    CREATED = "created"
    EXISTING = "existing"

    def __str__(self) -> str:
        return str(self.value)
