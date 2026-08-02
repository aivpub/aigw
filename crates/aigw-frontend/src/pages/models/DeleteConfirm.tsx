import { Button } from "@/components/ui/button";
import { useTranslation } from "react-i18next";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";

interface DeleteConfirmProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  modelName: string;
  onConfirm: () => void;
  loading?: boolean;
}

export function DeleteConfirm({
  open,
  onOpenChange,
  modelName,
  onConfirm,
  loading,
}: DeleteConfirmProps) {
  const { t } = useTranslation();

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-sm">
        <DialogHeader>
          <DialogTitle>{t("models.deleteDialog.title")}</DialogTitle>
          <DialogDescription>
            {t("models.deleteDialog.description", { modelName })}
          </DialogDescription>
        </DialogHeader>
        <DialogFooter>
          <Button
            variant="outline"
            onClick={() => onOpenChange(false)}
            disabled={loading}
          >
            {t("common.cancel")}
          </Button>
          <Button variant="destructive" onClick={onConfirm} disabled={loading}>
            {loading ? t("common.deleting") : t("common.delete")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
