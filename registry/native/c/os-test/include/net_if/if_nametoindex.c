#include <net/if.h>
#ifdef if_nametoindex
#undef if_nametoindex
#endif
unsigned (*foo)(const char *) = if_nametoindex;
int main(void) { return 0; }
