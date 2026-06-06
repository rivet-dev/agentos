#include <stdlib.h>
#ifdef atol
#undef atol
#endif
long (*foo)(const char *) = atol;
int main(void) { return 0; }
