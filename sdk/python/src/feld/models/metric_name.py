from enum import StrEnum


class MetricName(StrEnum):
    ACTIVE_TIME = "active_time"
    API_ERRORS = "api_errors"
    API_REQUEST_LATENCY_P95 = "api_request_latency_p95"
    CONTEXT_WINDOW_UTILIZATION = "context_window_utilization"
    ESTIMATED_COST = "estimated_cost"
    PROMPT_CACHE_HIT_RATE = "prompt_cache_hit_rate"
    TOKEN_USAGE = "token_usage"
    TOOL_CALLS = "tool_calls"

    def __str__(self) -> str:
        return str(self.value)
