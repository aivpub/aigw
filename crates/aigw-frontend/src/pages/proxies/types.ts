export interface ProxyItem {
  id: number;
  name: string;
  /** Redacted proxy URL: scheme://user:***@host:port */
  proxy_url: string;
  status: string; // active / inactive / expired
  expires_at: string | null;
  probe_result: Record<string, unknown>;
  created_at: string;
  updated_at: string;
  exit_ip?: string | null;
  country?: string | null;
  country_code?: string | null;
  latency_ms?: number | null;
  score?: number | null;
  grade?: string | null;
}

export interface ProxyListResponse {
  object: string;
  data: ProxyItem[];
  total_count?: number;
  page?: number;
  page_size?: number;
  total_pages?: number;
}

export interface QualityItem {
  target: string;
  status: string; // pass / warn / challenge / fail
  latency_ms?: number | null;
  cf_ray?: string | null;
  message: string;
}

export interface QualityResult {
  score: number;
  grade: string;
  overall_status: string;
  exit_ip?: string | null;
  country?: string | null;
  country_code?: string | null;
  base_latency_ms?: number | null;
  items: QualityItem[];
  last_check_at: string;
}
