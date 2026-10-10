import type { Metadata } from "next";
import "./globals.css";

export const metadata: Metadata = {
  title: "uniMCP",
  description: "Connect your AI app to your university.",
};

export default function RootLayout({ children }: LayoutProps<"/">) {
  return (
    // Browser extensions add attributes to <html> before React starts, which
    // React would otherwise report as a mismatch.
    <html lang="en" className="h-full antialiased" suppressHydrationWarning>
      <body className="min-h-full font-sans">{children}</body>
    </html>
  );
}
