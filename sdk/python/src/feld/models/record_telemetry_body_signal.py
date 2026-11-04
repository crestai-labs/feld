from enum import StrEnum


class RecordTelemetryBodySignal(StrEnum):
    LOGS = "logs"
    METRICS = "metrics"

    def __str__(self) -> str:
        return str(self.value)
