#include <pwd.h>
void foo(struct passwd* bar)
{
	char **qux = &bar->pw_name;
	(void) qux;
}
int main(void) { return 0; }
