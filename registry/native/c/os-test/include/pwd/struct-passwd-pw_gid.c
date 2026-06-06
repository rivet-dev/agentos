#include <pwd.h>
void foo(struct passwd* bar)
{
	gid_t *qux = &bar->pw_gid;
	(void) qux;
}
int main(void) { return 0; }
