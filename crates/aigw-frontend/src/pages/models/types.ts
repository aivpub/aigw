export interface ModelItem {
  model_id: string;
  model_name: string;
  litellm_params: Record<string, unknown>;
  model_info: Record<string, unknown>;
  created_at: string;
  created_by: string | null;
  updated_at: string;
  updated_by: string | null;
}

export interface ModelListResponse {
  object: string;
  data: ModelItem[];
  total_count?: number;
  page?: number;
  page_size?: number;
  total_pages?: number;
}

export interface DeletedModelListResponse {
  data: DeletedModelItem[];
  total_count?: number;
  page?: number;
  page_size?: number;
  total_pages?: number;
}

export interface DeletedModelItem {
  id: number;
  model_id: string;
  model_name: string;
  litellm_params: Record<string, unknown>;
  deleted_at: string;
}
