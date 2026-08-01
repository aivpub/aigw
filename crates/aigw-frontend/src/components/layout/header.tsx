import { Menu, LogOut, PanelLeftOpen, PanelLeftClose, Languages } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { useAuth } from "@/hooks/use-auth";

interface HeaderProps {
  onMenuClick: () => void;
  collapsed: boolean;
  onToggleCollapse: () => void;
}

export function Header({ onMenuClick, collapsed, onToggleCollapse }: HeaderProps) {
  const { t, i18n } = useTranslation();
  const { logout } = useAuth();
  const currentLang = i18n.language?.startsWith('zh') ? 'zh-CN' : 'en';

  const switchLanguage = (lang: string) => {
    i18n.changeLanguage(lang);
  };

  return (
    <header className="sticky top-0 z-30 flex h-14 items-center gap-4 border-b bg-background px-4 lg:px-6">
      <Button
        variant="ghost"
        size="icon"
        className="lg:hidden"
        onClick={onMenuClick}
      >
        <Menu className="h-5 w-5" />
      </Button>

      {/* Desktop sidebar toggle */}
      <Button
        variant="ghost"
        size="icon"
        className="hidden lg:flex"
        onClick={onToggleCollapse}
        title={collapsed ? t('header.expandSidebar') : t('header.collapseSidebar')}
      >
        {collapsed ? <PanelLeftOpen className="h-5 w-5" /> : <PanelLeftClose className="h-5 w-5" />}
      </Button>

      <div className="flex-1" />

      {/* Language Switcher */}
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button variant="ghost" size="sm" aria-label={t('header.switchLanguage')}>
            <Languages className="h-4 w-4" />
            <span className="ml-1.5 hidden sm:inline">{currentLang === 'zh-CN' ? '中文' : 'English'}</span>
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end">
          <DropdownMenuItem onClick={() => switchLanguage('zh-CN')}>
            中文 {currentLang === 'zh-CN' && '✓'}
          </DropdownMenuItem>
          <DropdownMenuItem onClick={() => switchLanguage('en')}>
            English {currentLang === 'en' && '✓'}
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>

      <Button variant="ghost" size="sm" onClick={logout}>
        <LogOut className="h-4 w-4 mr-2" />
        <span className="hidden sm:inline">{t('header.logout')}</span>
      </Button>
    </header>
  );
}
