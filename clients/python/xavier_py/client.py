import os
import warnings
import requests
import aiohttp
from typing import Optional, Dict, Any
from .models import (
    SearchResponse,
    RetrieveResponse,
    StatsResponse,
)

class XavierClient:
    """
    Official Python SDK for Xavier Memory API.
    Supports both synchronous (using requests) and asynchronous (using aiohttp) operations.
    """

    def __init__(
        self,
        base_url: str = "http://localhost:8006",
        token: Optional[str] = None
    ):
        self.base_url = base_url.rstrip("/")
        self.token = token or os.environ.get("XAVIER_TOKEN")

        if not self.token:
            warnings.warn(
                "No XAVIER_TOKEN set. Set the XAVIER_TOKEN environment variable or pass token= explicitly.",
                stacklevel=2
            )

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
        response = requests.post(url, json=payload, headers=self._get_headers(), timeout=30)
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
        response = requests.post(url, json=payload, headers=self._get_headers(), timeout=30)
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
        response = requests.post(url, json=payload, headers=self._get_headers(), timeout=30)
        response.raise_for_status()
        return RetrieveResponse(**response.json())

    def stats(self) -> StatsResponse:
        """Get memory statistics."""
        url = f"{self.base_url}/memory/stats"
        response = requests.get(url, headers=self._get_headers(), timeout=30)
        response.raise_for_status()
        return StatsResponse(**response.json())

    def delete(self, id: Optional[str] = None, path: Optional[str] = None) -> Dict[str, Any]:
        """Delete a memory entry by id or path."""
        if id is None and path is None:
            raise ValueError("Either 'id' or 'path' must be provided.")
        url = f"{self.base_url}/memory/delete"
        payload = {"id": id, "path": path}
        response = requests.post(url, json=payload, headers=self._get_headers(), timeout=30)
        # Xavier returns 404 with JSON body for not-found entries
        if response.status_code == 404:
            return response.json()
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
        if id is None and path is None:
            raise ValueError("Either 'id' or 'path' must be provided.")
        url = f"{self.base_url}/memory/delete"
        payload = {"id": id, "path": path}
        async with aiohttp.ClientSession() as session:
            async with session.post(url, json=payload, headers=self._get_headers()) as response:
                if response.status == 404:
                    return await response.json()
                response.raise_for_status()
                return await response.json()
