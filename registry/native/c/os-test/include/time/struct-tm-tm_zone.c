#include <time.h>
void foo(struct tm* bar)
{
	const char **qux = &bar->tm_zone;
	(void) qux;
}
int main(void) { return 0; }
