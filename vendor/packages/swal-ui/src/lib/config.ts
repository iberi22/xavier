let customGetXavierBaseUrl: () => string = () => "http://127.0.0.1:8006";

export function setXavierBaseUrlResolver(resolver: () => string) {
  customGetXavierBaseUrl = resolver;
}

export function getXavierBaseUrl(): string {
  return customGetXavierBaseUrl();
}
