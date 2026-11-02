use fixture_semconv::attrs;

fn main() {
    let route = String::from("/");
    let _ = attrs![http.request.method: "GET", http.route: route, http.response.status_code: 200];
}
