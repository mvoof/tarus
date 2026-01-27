import { emit } from "@tauri-apps/api/event";

export function MyComponent() {
    const handleClick = () => {
        emit("button-clicked", { timestamp: Date.now() });
    };
    
    return <button onClick={handleClick}>Click me</button>;
}
