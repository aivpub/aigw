import { useQuery } from "@tanstack/react-query";
import { apiGet } from "@/lib/api";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { Badge } from "@/components/ui/badge";
import { Skeleton } from "@/components/ui/skeleton";

interface ProxyModel {
  id: number;
  model_name: string;
  litellm_model_name: string;
  provider: string;
  model_type: string;
  is_active: boolean;
}

export function ModelsPage() {
  const { data: models, isLoading, error } = useQuery<ProxyModel[]>({
    queryKey: ["proxy-models"],
    queryFn: () => apiGet("/model/list"),
  });

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-2xl font-bold tracking-tight">Models</h1>
        <p className="text-sm text-muted-foreground">
          Proxy model configurations
        </p>
      </div>

      <Card>
        <CardHeader>
          <CardTitle>All Models</CardTitle>
        </CardHeader>
        <CardContent>
          {error ? (
            <p className="text-sm text-destructive">{(error as Error).message}</p>
          ) : (
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Model Name</TableHead>
                  <TableHead>Upstream Model</TableHead>
                  <TableHead>Provider</TableHead>
                  <TableHead>Type</TableHead>
                  <TableHead>Status</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {isLoading
                  ? Array.from({ length: 3 }).map((_, i) => (
                      <TableRow key={i}>
                        {Array.from({ length: 5 }).map((_, j) => (
                          <TableCell key={j}>
                            <Skeleton className="h-4 w-full" />
                          </TableCell>
                        ))}
                      </TableRow>
                    ))
                  : models?.map((m) => (
                      <TableRow key={m.id}>
                        <TableCell className="font-mono text-sm">{m.model_name}</TableCell>
                        <TableCell>{m.litellm_model_name}</TableCell>
                        <TableCell>{m.provider}</TableCell>
                        <TableCell>{m.model_type}</TableCell>
                        <TableCell>
                          <Badge variant={m.is_active ? "default" : "secondary"}>
                            {m.is_active ? "active" : "inactive"}
                          </Badge>
                        </TableCell>
                      </TableRow>
                    ))}
              </TableBody>
            </Table>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
