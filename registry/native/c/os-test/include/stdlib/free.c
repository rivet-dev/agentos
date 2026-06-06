#include <stdlib.h>
#ifdef free
#undef free
#endif
void (*foo)(void *) = free;
int main(void) { return 0; }
