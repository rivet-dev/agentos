#include <fcntl.h>
void foo(struct f_owner_ex* bar)
{
	int *qux = &bar->type;
	(void) qux;
}
int main(void) { return 0; }
