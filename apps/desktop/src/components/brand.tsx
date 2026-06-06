import { useT } from "../lib/i18n";
import { brandAssets } from "../lib/brand";

type BrandProps = {
  className?: string;
  variant?: "theme" | "white" | "color";
};

function BrandMarkAsset({ variant = "theme" }: Pick<BrandProps, "variant">) {
  if (variant === "white") {
    return <img className="brandmark-img" src={brandAssets.markWhite} alt="" draggable={false} />;
  }
  if (variant === "color") {
    return <img className="brandmark-img" src={brandAssets.markColor} alt="" draggable={false} />;
  }
  return (
    <>
      <img
        className="brandmark-img brandmark-img-light"
        src={brandAssets.markLight}
        alt=""
        draggable={false}
      />
      <img
        className="brandmark-img brandmark-img-dark"
        src={brandAssets.markDark}
        alt=""
        draggable={false}
      />
    </>
  );
}

export function BrandLogo({ className = "brand-logo", variant = "theme" }: BrandProps) {
  const t = useT();
  return (
    <span className={className} aria-label={t("brand.logoAriaLabel")}>
      <span className="brand-logo-mark brandmark" aria-hidden="true">
        <BrandMarkAsset variant={variant} />
      </span>
    </span>
  );
}

export function BrandMark({ className = "brand-mark", variant = "theme" }: BrandProps) {
  return (
    <span className={`${className} brandmark`} aria-hidden="true">
      <BrandMarkAsset variant={variant} />
    </span>
  );
}

export function LogoLockup() {
  return (
    <div className="logo-lockup">
      <BrandLogo variant="theme" />
    </div>
  );
}
