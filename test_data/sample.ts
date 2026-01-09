
export const API_URL = "https://api.example.com";

export function fetchData(id: string): Promise<any> {
    return fetch(`${API_URL}/data/${id}`);
}

export class UserManager {
    constructor(private db: any) { }

    async getUser(id: string) {
        return this.db.findUser(id);
    }
}
