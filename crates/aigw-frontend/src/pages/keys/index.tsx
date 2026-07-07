import { useQuery } from "@tanstack/react-query";
import { apiGet } from "@/lib/api";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { Skeleton } from "@/components/ui/skeleton";
import { Plus } from "lucide-react";
import { Button } from "@/components/ui/button";

interface VirtualKey {
  id: number;
  key_alias: string;
  key_type: string;
  model_group: string;
  spend: number;
  max_budget: number | null;
  created_at: string;
}

export function KeysPage() {
  const { data: keys, isLoading, error } = useQuery<VirtualKey[]>({
    queryKey: ["virtual-keys"],
    queryFn: () => apiGet("/key/list"),
  });

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-bold tracking-tight">API Keys</h1>
          <p className="text-sm text-muted-foreground">
            Manage virtual keys for LLM access
          </p>
        </div>
        <Button>
          <Plus className="h-4 w-4" />
          New Key
        </Button>
      </div>

      <Card>
        <CardHeader>
          <CardTitle>All Keys</CardTitle>
        </CardHeader>
        <CardContent>
          {error ? (
            <p className="text-sm text-destructive">{(error as Error).message}</p>
          ) : (
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Alias</TableHead>
                  <TableHead>Type</TableHead>
                  <TableHead>Model Group</TableHead>
                  <TableHead className="text-right">Spend</TableHead>
                  <TableHead className="text-right">Budget</TableHead>
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
                  : keys?.map((key) => (
                      <TableRow key={key.id}>
                        <TableCell className="font-mono text-sm">{key.key_alias}</TableCell>
                        <TableCell>{key.key_type}</TableCell>
                        <TableCell>{key.model_group}</TableCell>
                        <TableCell className="text-right">${key.spend.toFixed(4)}</TableCell>
                        <TableCell className="text-right">
                          {key.max_budget != null ? `$${key.max_budget.toFixed(2)}` : "—"}
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
