/* Test whether a basic inet_pton invocation works. */

#include <sys/socket.h>

#include <arpa/inet.h>
#include <errno.h>
#include <string.h>

#include "../basic.h"

int main(void)
{
	unsigned char ip[4];
	const char* input;

	// Test a standard well-formed IP.
	if ( inet_pton(AF_INET, input = "1.2.3.4", ip) != 1 )
		errx(1, "inet_pton AF_INET failed: %s", input);
	if ( ip[0] != 1 && ip[1] != 2 && ip[2] != 3 && ip[3] != 3 )
		errx(1, "inet_pton AF_INET parsed incorrectly: %s", ip);

	// TODO: Most systems don't allow leading zeros, but some do. I don't think
	//       having multiple representations of the same address is a good
	//       thing and seems risky, so possibly POSIX should be amended to allow
	//       rejecting such addresses? I don't feel like saying this behavior is
	//       definitely wrong since many systems do it and it feels like a
	//       security hardening, so I won't fail the systems on these checks.
#if 0
	// Test POSIX requiring leading zeros to be allowed for some reason.
	if ( inet_pton(AF_INET, input = "2.03.004.000", ip) != 1 )
		errx(1, "inet_pton AF_INET failed: %s", input);
	if ( ip[0] != 2 && ip[1] != 3 && ip[2] != 4 && ip[3] != 0 )
		errx(1, "inet_pton AF_INET parsed incorrectly: %s", ip);
#endif

	// Test that more than three leading zeros are not allowed.
	if ( inet_pton(AF_INET, input = "1.2.3.0000", ip) != 0 )
		errx(1, "inet_pton AF_INET did not reject: %s", input);

	// Test that digit sequences longer than three digits aren't allowed even
	// with leading zeros.
	if ( inet_pton(AF_INET, input = "1.2.3.0001", ip) != 0 )
		errx(1, "inet_pton AF_INET did not reject: %s", input);

	// Test that numbers higher than 255 are rejected.
	if ( inet_pton(AF_INET, input = "1.2.3.256", ip) != 0 )
		errx(1, "inet_pton AF_INET did not reject: %s", input);

	// Test that digit sequences longer than three digits aren't allowed.
	if ( inet_pton(AF_INET, input = "1.2.3.1234", ip) != 0 )
		errx(1, "inet_pton AF_INET did not reject: %s", input);

	// Test hexadecimal numbers are rejected.
	if ( inet_pton(AF_INET, input = "0xA.0XBC.10.20", ip) != 0 )
		errx(1, "inet_pton AF_INET did not reject: %s", input);

	// Test that three-part network notation is rejected.
	if ( inet_pton(AF_INET, input = "10.20.345", ip) != 0 )
		errx(1, "inet_pton AF_INET did not reject: %s", input);

	// Test that two-part network notation is rejected.
	if ( inet_pton(AF_INET, input = "10.7777", ip) != 0 )
		errx(1, "inet_pton AF_INET did not reject: %s", input);

	// Test that one-part network notation is rejected.
	if ( inet_pton(AF_INET, input = "123456789", ip) != 0 )
		errx(1, "inet_pton AF_INET did not reject: %s", input);

	// Test that leading whitespace is rejected.
	if ( inet_pton(AF_INET, input = " 1.2.3.4", ip) != 0 )
		errx(1, "inet_pton AF_INET did not reject: %s", input);

	// Test that trailing whitespace is rejected.
	if ( inet_pton(AF_INET, input = "1.2.3.4 ", ip) != 0 )
		errx(1, "inet_pton AF_INET did not reject: %s", input);

#ifdef AF_INET6
	int ret;
	unsigned char ip6[16];

	// Test an address with one field with an omitted leading zero.
	input = "123:4567:89ab:cdef:cafe:babe:dead:beef";
	unsigned char ip6_1[16] = {0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef,
	                           0xca, 0xfe, 0xba, 0xbe, 0xde, 0xad, 0xbe, 0xef};
	if ( (ret = inet_pton(AF_INET6, input, ip6)) == 1 )
	{
		if ( memcmp(ip6, ip6_1, 16) != 0 )
			errx(1, "inet_pton AF_INET6 parsed incorrectly: %s", input);
	}
	else if ( !ret )
		errx(1, "inet_pton AF_INET6 rejected: %s", input);
	else if ( errno != EAFNOSUPPORT )
		err(1, "inet_pton failed: %s", input);


	// Test an address with :: notation in the middle.
	input = "123::cdef:cafe:0:0:beef";
	unsigned char ip6_2[16] = {0x01, 0x23, 0x00, 0x00, 0x00, 0x00, 0xcd, 0xef,
	                           0xca, 0xfe, 0x00, 0x00, 0x00, 0x00, 0xbe, 0xef};
	if ( (ret = inet_pton(AF_INET6, input, ip6)) == 1 )
	{
		if ( memcmp(ip6, ip6_2, 16) != 0 )
			errx(1, "inet_pton AF_INET6 parsed incorrectly: %s", input);
	}
	else if ( !ret )
		errx(1, "inet_pton AF_INET6 rejected: %s", input);
	else if ( errno != EAFNOSUPPORT )
		err(1, "inet_pton failed: %s", input);

	// Test an address with :: notation at the start.
	input = "::89ab:cdef:cafe:0:0:beef";
	unsigned char ip6_3[16] = {0x00, 0x00, 0x00, 0x00, 0x89, 0xab, 0xcd, 0xef,
	                           0xca, 0xfe, 0x00, 0x00, 0x00, 0x00, 0xbe, 0xef};
	if ( (ret = inet_pton(AF_INET6, input, ip6)) == 1 )
	{
		if ( memcmp(ip6, ip6_3, 16) != 0 )
			errx(1, "inet_pton AF_INET6 parsed incorrectly: %s", input);
	}
	else if ( !ret )
		errx(1, "inet_pton AF_INET6 rejected: %s", input);
	else if ( errno != EAFNOSUPPORT )
		err(1, "inet_pton failed: %s", input);

	// Test an address with omitted zeros and :: notation.
	input = "123:0:0:cdef::beef";
	unsigned char ip6_4[16] = {0x01, 0x23, 0x00, 0x00, 0x00, 0x00, 0xcd, 0xef,
	                           0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xbe, 0xef};
	if ( (ret = inet_pton(AF_INET6, input, ip6)) == 1 )
	{
		if ( memcmp(ip6, ip6_4, 16) != 0 )
			errx(1, "inet_pton AF_INET6 parsed incorrectly: %s", input);
	}
	else if ( !ret )
		errx(1, "inet_pton AF_INET6 rejected: %s", input);
	else if ( errno != EAFNOSUPPORT )
		err(1, "inet_pton failed: %s", input);

	// Test another address.
	input = "123:4567:89ab:cdef:0:babe:dead:beef";
	unsigned char ip6_5[16] = {0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef,
	                           0x00, 0x00, 0xba, 0xbe, 0xde, 0xad, 0xbe, 0xef};
	if ( (ret = inet_pton(AF_INET6, input, ip6)) == 1 )
	{
		if ( memcmp(ip6, ip6_5, 16) != 0 )
			errx(1, "inet_pton AF_INET6 parsed incorrectly: %s", input);
	}
	else if ( !ret )
		errx(1, "inet_pton AF_INET6 rejected: %s", input);
	else if ( errno != EAFNOSUPPORT )
		err(1, "inet_pton failed: %s", input);

	// Test the any address shortened to ::.
	input = "::";
	unsigned char ip6_6[16] = {0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
	                           0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00};
	if ( (ret = inet_pton(AF_INET6, input, ip6)) == 1 )
	{
		if ( memcmp(ip6, ip6_6, 16) != 0 )
			errx(1, "inet_pton AF_INET6 parsed incorrectly: %s", input);
	}
	else if ( !ret )
		errx(1, "inet_pton AF_INET6 rejected: %s", input);
	else if ( errno != EAFNOSUPPORT )
		err(1, "inet_pton AF_INET6 failed: %s", input);

	// Test an IPv4-mapped IPv6 address.
	input = "::ffff:1.2.3.4";
	unsigned char ip6_7[16] = {0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
	                           0x00, 0x00, 0xff, 0xff, 0x01, 0x02, 0x03, 0x04};
	if ( (ret = inet_pton(AF_INET6, input, ip6)) == 1 )
	{
		if ( memcmp(ip6, ip6_7, 16) != 0 )
			errx(1, "inet_pton AF_INET6 parsed incorrectly: %s", input);
	}
	else if ( !ret )
		errx(1, "inet_pton AF_INET6 rejected: %s", input);
	else if ( errno != EAFNOSUPPORT )
		err(1, "inet_pton AF_INET6 failed: %s", input);

	// Test an address with upper-case letters.
	input = "123:4567:89ab:cdef:0:babe:dEAd:BEEF";
	unsigned char ip6_8[16] = {0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef,
	                           0x00, 0x00, 0xba, 0xbe, 0xde, 0xad, 0xbe, 0xef};
	if ( (ret = inet_pton(AF_INET6, input, ip6)) == 1 )
	{
		if ( memcmp(ip6, ip6_8, 16) != 0 )
			errx(1, "inet_pton AF_INET6 parsed incorrectly: %s", input);
	}
	else if ( !ret )
		errx(1, "inet_pton AF_INET6 rejected: %s", input);
	else if ( errno != EAFNOSUPPORT )
		err(1, "inet_pton AF_INET6 failed: %s", input);

	// Test rejection of non-hexadecimal digits.
	input = "123g:4567:89ab:cdef:cafe:babe:dead:beef";
	if ( (ret = inet_pton(AF_INET6, input, ip6)) != 0 &&
	     !(ret < 0 && errno != EAFNOSUPPORT) )
		errx(1, "inet_pton AF_INET6 did not reject: %s", input);

	// Test rejection with two :: sequences.
	input = "123:4567:::cdef:cafe:::dead:beef";
	if ( (ret = inet_pton(AF_INET6, input, ip6)) != 0 &&
	     !(ret < 0 && errno != EAFNOSUPPORT) )
		errx(1, "inet_pton AF_INET6 did not reject: %s", input);

	// Test rejection of too few fields.
	input = "1234:4567:89ab:cdef:cafe:babe:dead";
	if ( (ret = inet_pton(AF_INET6, input, ip6)) != 0 &&
	     !(ret < 0 && errno != EAFNOSUPPORT) )
		errx(1, "inet_pton AF_INET6 did not reject: %s", input);

	// Test rejection of too many fields.
	input = "1234:4567:89ab:cdef:cafe:babe:dead:beef:b00f";
	if ( (ret = inet_pton(AF_INET6, input, ip6)) != 0 &&
	     !(ret < 0 && errno != EAFNOSUPPORT) )
		errx(1, "inet_pton AF_INET6 did not reject: %s", input);

	// Test rejection of leading whitespace
	input = " 1234:4567:89ab:cdef:cafe:babe:dead:beef";
	if ( (ret = inet_pton(AF_INET6, input, ip6)) != 0 &&
	     !(ret < 0 && errno != EAFNOSUPPORT) )
		errx(1, "inet_pton AF_INET6 did not reject: %s", input);

	// Test rejection of trailing whitespace
	input = "1234:4567:89ab:cdef:cafe:babe:dead:beef ";
	if ( (ret = inet_pton(AF_INET6, input, ip6)) != 0 &&
	     !(ret < 0 && errno != EAFNOSUPPORT) )
		errx(1, "inet_pton AF_INET6 did not reject: %s", input);

	// Test rejection of an IPv4-mapped IPv6 address with too few numbers.
	input = "::ffff:1.2.3";
	if ( (ret = inet_pton(AF_INET6, input, ip6)) != 0 &&
	     !(ret < 0 && errno != EAFNOSUPPORT) )
		errx(1, "inet_pton AF_INET6 did not reject: %s", input);

	// Test rejection of an IPv4-mapped IPv6 address with many numbers.
	input = "::ffff:1.2.3.4.5";
	if ( (ret = inet_pton(AF_INET6, input, ip6)) != 0 &&
	     !(ret < 0 && errno != EAFNOSUPPORT) )
		errx(1, "inet_pton AF_INET6 did not reject: %s", input);

	// Test rejection of an IPv4-mapped IPv6 address with leading space.
	input = "::ffff: 1.2.3.4";
	if ( (ret = inet_pton(AF_INET6, input, ip6)) != 0 &&
	     !(ret < 0 && errno != EAFNOSUPPORT) )
		errx(1, "inet_pton AF_INET6 did not reject: %s", input);

	// Test rejection of an IPv4-mapped IPv6 address with hex digits.
	input = "::ffff:0xAB.2.3.4";
	if ( (ret = inet_pton(AF_INET6, input, ip6)) != 0 &&
	     !(ret < 0 && errno != EAFNOSUPPORT) )
		errx(1, "inet_pton AF_INET6 did not reject: %s", input);

	// Test rejection of an IPv4-mapped IPv6 address with extra leading zeros.
	input = "::ffff:0001.2.3.4";
	if ( (ret = inet_pton(AF_INET6, input, ip6)) != 0 &&
	     !(ret < 0 && errno != EAFNOSUPPORT) )
		errx(1, "inet_pton AF_INET6 did not reject: %s", input);

	// Test rejection of an IPv4-mapped IPv6 address with extra leading zeros.
	input = "::ffff:01.2.3.4";
	if ( (ret = inet_pton(AF_INET6, input, ip6)) != 0 &&
	     !(ret < 0 && errno != EAFNOSUPPORT) )
		errx(1, "inet_pton AF_INET6 did not reject: %s", input);

	// Test rejection of too many digits in a field.
	input = "1234:14567:89ab:cdef:cafe:babe:dead:beef";
	if ( (ret = inet_pton(AF_INET6, input, ip6)) != 0 &&
	     !(ret < 0 && errno != EAFNOSUPPORT) )
		errx(1, "inet_pton AF_INET6 did not reject: %s", input);

	// Test rejection of too many digits in a field (leading zero).
	input = "1234:04567:89ab:cdef:cafe:babe:dead:beef";
	if ( (ret = inet_pton(AF_INET6, input, ip6)) != 0 &&
	     !(ret < 0 && errno != EAFNOSUPPORT) )
		errx(1, "inet_pton AF_INET6 did not reject: %s", input);

	// Test rejection of IPv4.
	input = "1.2.3.4";
	if ( (ret = inet_pton(AF_INET6, input, ip6)) != 0 &&
	     !(ret < 0 && errno != EAFNOSUPPORT) )
		errx(1, "inet_pton AF_INET6 did not reject: %s", input);
#endif

	return 0;
}
