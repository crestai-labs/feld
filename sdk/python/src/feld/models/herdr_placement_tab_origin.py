from enum import StrEnum


class HerdrPlacementTabOrigin(StrEnum):
    CREATED = "created"
    EXISTING = "existing"

    def __str__(self) -> str:
        return str(self.value)
