#include <net/if.h>
#ifdef if_freenameindex
#undef if_freenameindex
#endif
void (*foo)(struct if_nameindex *) = if_freenameindex;
int main(void) { return 0; }
