#include <pwd.h>
void foo(struct passwd* bar)
{
	char **qux = &bar->pw_dir;
	(void) qux;
}
int main(void) { return 0; }
