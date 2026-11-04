from enum import StrEnum


class HerdrPlacementEndpointKind(StrEnum):
    PATH = "path"
    SESSION = "session"

    def __str__(self) -> str:
        return str(self.value)
