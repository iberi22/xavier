import os
import requests
import aiohttp
import asyncio
from typing import Optional, List, Dict, Any, Union
from .models import (
    SearchResponse,
    RetrieveResponse,
    StatsResponse,
    GraphResponse
)

class XavierClient:
    """
    Official Python SDK for Xavier Memory API.
    Supports both synchronous (using requests) and asynchronous (using aiohttp) operations.
    """

    def __init__(
        self,
        base_url: str = "http://localhost:8080",
        token: Optional[str] = None
    ):
        self.base_url = base_url.rstrip("/")
        self.token = token or os.environ.get("XAVIER_TOKEN")

        if not self.token:
            # Fallback to dev-token if not provided and not in production
            self.token = "dev-token"

    def _get_headers(self) -> Dict[str, str]:
        return {
            "X-Xavier-Token": self.token,
            "Content-Type": "application/json"
        }

    # --- Synchronous Methods ---

    def add(
        self,
        content: str,
        path: Optional[str] = None,
        metadata: Optional[Dict[str, Any]] = None,
        **kwargs
    ) -> Dict[str, Any]:
        """Add a document to memory."""
        url = f"{self.base_url}/memory/add"
        payload = {
            "content": content,
            "path": path,
            "metadata": metadata,
            **kwargs
        }
        response = requests.post(url, json=payload, headers=self._get_headers())
        response.raise_for_status()
        return response.json()

    def search(
        self,
        query: str,
        limit: int = 10,
        filters: Optional[Dict[str, Any]] = None
    ) -> SearchResponse:
        """Search memory with semantic + lexical hybrid search."""
        url = f"{self.base_url}/memory/search"
        payload = {
            "query": query,
            "limit": limit,
            "filters": filters
        }
        response = requests.post(url, json=payload, headers=self._get_headers())
        response.raise_for_status()
        return SearchResponse(**response.json())

    def retrieve(
        self,
        query: str,
        limit: int = 10,
        **kwargs
    ) -> RetrieveResponse:
        """Perform multi-layer memory retrieval."""
        url = f"{self.base_url}/memory/retrieve"
        payload = {
            "query": query,
            "limit": limit,
            **kwargs
        }
        response = requests.post(url, json=payload, headers=self._get_headers())
        response.raise_for_status()
        return RetrieveResponse(**response.json())

    def stats(self) -> StatsResponse:
        """Get memory statistics."""
        url = f"{self.base_url}/memory/stats"
        response = requests.get(url, headers=self._get_headers())
        response.raise_for_status()
        return StatsResponse(**response.json())

    def delete(self, id: Optional[str] = None, path: Optional[str] = None) -> Dict[str, Any]:
        """Delete a memory entry by id or path."""
        url = f"{self.base_url}/memory/delete"
        payload = {"id": id, "path": path}
        response = requests.post(url, json=payload, headers=self._get_headers())
        response.raise_for_status()
        return response.json()

    # --- Asynchronous Methods ---

    async def add_async(
        self,
        content: str,
        path: Optional[str] = None,
        metadata: Optional[Dict[str, Any]] = None,
        **kwargs
    ) -> Dict[str, Any]:
        """Add a document to memory (Async)."""
        url = f"{self.base_url}/memory/add"
        payload = {
            "content": content,
            "path": path,
            "metadata": metadata,
            **kwargs
        }
        async with aiohttp.ClientSession() as session:
            async with session.post(url, json=payload, headers=self._get_headers()) as response:
                response.raise_for_status()
                return await response.json()

    async def search_async(
        self,
        query: str,
        limit: int = 10,
        filters: Optional[Dict[str, Any]] = None
    ) -> SearchResponse:
        """Search memory (Async)."""
        url = f"{self.base_url}/memory/search"
        payload = {
            "query": query,
            "limit": limit,
            "filters": filters
        }
        async with aiohttp.ClientSession() as session:
            async with session.post(url, json=payload, headers=self._get_headers()) as response:
                response.raise_for_status()
                data = await response.json()
                return SearchResponse(**data)

    async def retrieve_async(
        self,
        query: str,
        limit: int = 10,
        **kwargs
    ) -> RetrieveResponse:
        """Multi-layer retrieval (Async)."""
        url = f"{self.base_url}/memory/retrieve"
        payload = {
            "query": query,
            "limit": limit,
            **kwargs
        }
        async with aiohttp.ClientSession() as session:
            async with session.post(url, json=payload, headers=self._get_headers()) as response:
                response.raise_for_status()
                data = await response.json()
                return RetrieveResponse(**data)

    async def stats_async(self) -> StatsResponse:
        """Get stats (Async)."""
        url = f"{self.base_url}/memory/stats"
        async with aiohttp.ClientSession() as session:
            async with session.get(url, headers=self._get_headers()) as response:
                response.raise_for_status()
                data = await response.json()
                return StatsResponse(**data)

    async def delete_async(self, id: Optional[str] = None, path: Optional[str] = None) -> Dict[str, Any]:
        """Delete entry (Async)."""
        url = f"{self.base_url}/memory/delete"
        payload = {"id": id, "path": path}
        async with aiohttp.ClientSession() as session:
            async with session.post(url, json=payload, headers=self._get_headers()) as response:
                response.raise_for_status()
                return await response.json()
