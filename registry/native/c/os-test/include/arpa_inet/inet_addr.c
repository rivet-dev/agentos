/*[OB]*/
#include <arpa/inet.h>
#ifdef inet_addr
#undef inet_addr
#endif
in_addr_t (*foo)(const char *) = inet_addr;
int main(void) { return 0; }
