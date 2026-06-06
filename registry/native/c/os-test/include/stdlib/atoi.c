#include <stdlib.h>
#ifdef atoi
#undef atoi
#endif
int (*foo)(const char *) = atoi;
int main(void) { return 0; }
