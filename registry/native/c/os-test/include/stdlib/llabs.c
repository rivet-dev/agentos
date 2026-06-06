#include <stdlib.h>
#ifdef llabs
#undef llabs
#endif
long long (*foo)(long long) = llabs;
int main(void) { return 0; }
