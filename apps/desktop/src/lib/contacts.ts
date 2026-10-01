export type ContactNumber = { label: string; number: string };
export type Contact = {
  id: string;
  name: string;
  company: string;
  numbers: ContactNumber[];
  email: string;
  user_id: string | null;
};
export type Folder = { id: string; name: string };

export function initials(name: string) {
  return name.split(/\s+/).filter(Boolean).slice(0, 2).map((w) => w[0]).join("").toUpperCase() || "?";
}
