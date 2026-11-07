from enum import StrEnum


class ActorLabelBy(StrEnum):
    ACTOR = "actor"
    ADMIN = "admin"

    def __str__(self) -> str:
        return str(self.value)
