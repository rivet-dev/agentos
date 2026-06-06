#include <stdlib.h>
#ifdef mkstemp
#undef mkstemp
#endif
int (*foo)(char *) = mkstemp;
int main(void) { return 0; }
