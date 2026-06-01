import pytest
import os
from unittest.mock import patch, MagicMock
from xavier_py import XavierClient, SearchResponse, StatsResponse

@pytest.fixture
def client():
    return XavierClient(base_url="http://localhost:8080", token="test-token")

def test_client_init_token():
    with patch.dict(os.environ, {"XAVIER_TOKEN": "env-token"}):
        client = XavierClient()
        assert client.token == "env-token"

def test_get_headers(client):
    headers = client._get_headers()
    assert headers["X-Xavier-Token"] == "test-token"
    assert headers["Content-Type"] == "application/json"

@patch("requests.post")
def test_add(mock_post, client):
    mock_post.return_value.json.return_value = {"status": "ok", "id": "123"}
    mock_post.return_value.status_code = 200

    result = client.add("test content", path="test/path")

    assert result["status"] == "ok"
    assert result["id"] == "123"
    mock_post.assert_called_once()

@patch("requests.post")
def test_search(mock_post, client):
    mock_post.return_value.json.return_value = {
        "status": "ok",
        "query": "test query",
        "results": [
            {"id": "1", "path": "p1", "content": "c1", "metadata": {}}
        ]
    }
    mock_post.return_value.status_code = 200

    response = client.search("test query")

    assert isinstance(response, SearchResponse)
    assert response.status == "ok"
    assert len(response.results) == 1
    assert response.results[0].content == "c1"

@patch("requests.get")
def test_stats(mock_get, client):
    mock_get.return_value.json.return_value = {
        "status": "ok",
        "workspace_id": "default",
        "version": "0.6.1-beta"
    }
    mock_get.return_value.status_code = 200

    response = client.stats()

    assert isinstance(response, StatsResponse)
    assert response.version == "0.6.1-beta"

@pytest.mark.asyncio
async def test_add_async(client):
    with patch("aiohttp.ClientSession.post") as mock_post:
        mock_resp = MagicMock()
        mock_resp.status = 200

        # Async mock for .json()
        async def mock_json():
            return {"status": "ok", "id": "async-123"}
        mock_resp.json = mock_json

        mock_resp.__aenter__.return_value = mock_resp
        mock_post.return_value = mock_resp

        result = await client.add_async("async content")
        assert result["id"] == "async-123"
