#include <stdlib.h>
#ifdef mkdtemp
#undef mkdtemp
#endif
char *(*foo)(char *) = mkdtemp;
int main(void) { return 0; }
