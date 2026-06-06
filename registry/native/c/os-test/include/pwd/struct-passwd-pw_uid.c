#include <pwd.h>
void foo(struct passwd* bar)
{
	uid_t *qux = &bar->pw_uid;
	(void) qux;
}
int main(void) { return 0; }
