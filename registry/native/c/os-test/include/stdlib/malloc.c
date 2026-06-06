#include <stdlib.h>
#ifdef malloc
#undef malloc
#endif
void *(*foo)(size_t) = malloc;
int main(void) { return 0; }
