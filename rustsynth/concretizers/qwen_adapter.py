"""
Qwen LLM adapter using the DashScope Python SDK.

Configuration via environment variables:
  DASHSCOPE_API_KEY — API key (required)
  QWEN_MODEL       — Model name (default: qwen-plus)

Install:
  pip install dashscope
"""

from __future__ import annotations

import json
import logging
import os
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Optional

logger = logging.getLogger(__name__)


@dataclass
class QwenResponse:
    content: str
    model: str = ""
    usage_tokens: int = 0
    latency_ms: float = 0.0
    raw: dict = field(default_factory=dict)


class QwenAdapter:
    """Qwen adapter using DashScope SDK (dashscope.Generation)."""

    def __init__(
        self,
        api_key: str,
        model: str = "qwen-plus",
        log_dir: Optional[Path] = None,
    ):
        self.api_key = api_key
        self.model = model
        self.log_dir = log_dir or Path("results/qwen_logs")
        self.log_dir.mkdir(parents=True, exist_ok=True)
        self._call_count = 0

    @classmethod
    def from_env(cls) -> QwenAdapter:
        api_key = os.environ.get("DASHSCOPE_API_KEY", "")
        if not api_key:
            raise ValueError(
                "DASHSCOPE_API_KEY environment variable not set. "
                "Get your key from https://dashscope.console.aliyun.com/"
            )
        model = os.environ.get("QWEN_MODEL", "qwen3.5-plus")
        return cls(api_key=api_key, model=model)

    @classmethod
    def is_available(cls) -> bool:
        return bool(os.environ.get("DASHSCOPE_API_KEY"))

    def complete(
        self,
        prompt: str,
        system_prompt: str = "You are a Rust programming expert.",
        temperature: float = 0.2,
        max_tokens: int = 2048,
    ) -> str:
        response = self._call_api(prompt, system_prompt, temperature, max_tokens)
        return response.content

    def _call_api(
        self,
        prompt: str,
        system_prompt: str,
        temperature: float,
        max_tokens: int,
    ) -> QwenResponse:
        from dashscope import Generation

        self._call_count += 1
        call_id = self._call_count

        messages = [
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": prompt},
        ]

        t0 = time.time()
        try:
            response = Generation.call(
                api_key=self.api_key,
                model=self.model,
                messages=messages,
                temperature=temperature,
                max_tokens=max_tokens,
                result_format="message",
            )
        except Exception as e:
            logger.error("DashScope API request failed: %s", e)
            self._log_call(call_id, prompt, "", {"error": str(e)})
            raise

        latency = (time.time() - t0) * 1000

        if response.status_code != 200:
            error_msg = (
                f"HTTP {response.status_code}: "
                f"code={response.code}, message={response.message}"
            )
            logger.error("DashScope API error: %s", error_msg)
            self._log_call(call_id, prompt, "", {"error": error_msg})
            raise RuntimeError(error_msg)

        content = response.output.choices[0].message.content
        usage = getattr(response.usage, "total_tokens", 0)

        raw_dict = {}
        try:
            raw_dict = {
                "status_code": response.status_code,
                "model": getattr(response.output, "model", self.model),
                "usage": {
                    "total_tokens": usage,
                    "input_tokens": getattr(response.usage, "input_tokens", 0),
                    "output_tokens": getattr(response.usage, "output_tokens", 0),
                },
            }
        except Exception:
            pass

        result = QwenResponse(
            content=content,
            model=raw_dict.get("model", self.model),
            usage_tokens=usage,
            latency_ms=latency,
            raw=raw_dict,
        )

        self._log_call(call_id, prompt, content, {
            "model": result.model,
            "tokens": usage,
            "latency_ms": latency,
        })

        return result

    def _log_call(
        self, call_id: int, prompt: str, response: str, meta: dict
    ) -> None:
        log_file = self.log_dir / f"call_{call_id:04d}.json"
        record = {
            "call_id": call_id,
            "prompt": prompt[:5000],
            "response": response[:5000],
            "meta": meta,
        }
        try:
            with open(log_file, "w") as f:
                json.dump(record, f, indent=2, ensure_ascii=False)
        except Exception:
            pass
