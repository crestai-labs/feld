from enum import StrEnum


class TmuxPlacementWindowOrigin(StrEnum):
    CREATED = "created"
    EXISTING = "existing"

    def __str__(self) -> str:
        return str(self.value)
