#include <stdlib.h>
#ifdef atoll
#undef atoll
#endif
long long (*foo)(const char *) = atoll;
int main(void) { return 0; }
