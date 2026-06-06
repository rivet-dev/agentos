#include <stdlib.h>
#ifdef realloc
#undef realloc
#endif
void *(*foo)(void *, size_t) = realloc;
int main(void) { return 0; }
