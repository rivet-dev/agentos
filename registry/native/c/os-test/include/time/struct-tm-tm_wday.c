#include <time.h>
void foo(struct tm* bar)
{
	int *qux = &bar->tm_wday;
	(void) qux;
}
int main(void) { return 0; }
