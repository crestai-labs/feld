from enum import StrEnum


class RuntimeQuotaSeriesItemWindow(StrEnum):
    FIVE_HOUR = "five_hour"
    SEVEN_DAY = "seven_day"

    def __str__(self) -> str:
        return str(self.value)
