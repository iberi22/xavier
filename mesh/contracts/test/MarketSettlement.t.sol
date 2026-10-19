// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "../contracts/MarketSettlement.sol";

// Minimal cheatcode interface matching foundry's VM
interface Vm {
    function prank(address msgSender) external;
    function deal(address who, uint256 newBalance) external;
    function warp(uint256 newTimestamp) external;
    function expectRevert(bytes calldata revertData) external;
}

contract MockERC20 {
    mapping(address => uint256) public balanceOf;
    mapping(address => mapping(address => uint256)) public allowance;

    function mint(address to, uint256 amount) external {
        balanceOf[to] += amount;
    }

    function approve(address spender, uint256 amount) external returns (bool) {
        allowance[msg.sender][spender] = amount;
        return true;
    }

    function transfer(address to, uint256 amount) external returns (bool) {
        require(balanceOf[msg.sender] >= amount, "Insufficient balance");
        balanceOf[msg.sender] -= amount;
        balanceOf[to] += amount;
        return true;
    }

    function transferFrom(address from, address to, uint256 amount) external returns (bool) {
        require(balanceOf[from] >= amount, "Insufficient balance");
        require(allowance[from][msg.sender] >= amount, "Insufficient allowance");
        balanceOf[from] -= amount;
        allowance[from][msg.sender] -= amount;
        balanceOf[to] += amount;
        return true;
    }
}

contract MarketSettlementTest {
    Vm internal constant vm = Vm(address(uint160(uint256(keccak256("hevm cheat code")))));

    MarketSettlement internal settlement;
    MockERC20 internal token;

    address internal constant OWNER = address(0xAA1);
    address internal constant FEE_RECIPIENT = address(0xFFE);
    address internal constant BUYER = address(0xB1);
    address internal constant SELLER = address(0x5E);
    address internal constant STRANGER = address(0x999);

    bytes32 internal constant ORDER_1 = keccak256("order-1");
    bytes32 internal constant LISTING_1 = keccak256("listing-1");

    receive() external payable {}

    function setUp() public {
        vm.prank(OWNER);
        settlement = new MarketSettlement(FEE_RECIPIENT);
        token = new MockERC20();

        vm.deal(BUYER, 100 ether);
        token.mint(BUYER, 10000);
    }

    function test_initialState() public view {
        require(settlement.owner() == OWNER, "Owner mismatch");
        require(settlement.feeRecipient() == FEE_RECIPIENT, "Fee recipient mismatch");
    }

    function test_createNativeEscrow() public {
        vm.prank(BUYER);
        settlement.createEscrow{value: 1 ether}(
            ORDER_1,
            LISTING_1,
            SELLER,
            address(0),
            1 ether,
            250, // 2.5%
            2 hours
        );

        MarketSettlement.EscrowOrder memory order = settlement.getEscrow(ORDER_1);
        require(order.buyer == BUYER, "Buyer mismatch");
        require(order.seller == SELLER, "Seller mismatch");
        require(order.amount == 1 ether, "Amount mismatch");
        require(order.feeBps == 250, "Fee bps mismatch");
        require(order.state == MarketSettlement.EscrowState.Deposited, "State not deposited");
    }

    function test_completeNativeEscrowByBuyer() public {
        vm.prank(BUYER);
        settlement.createEscrow{value: 1 ether}(
            ORDER_1,
            LISTING_1,
            SELLER,
            address(0),
            1 ether,
            250, // 2.5%
            2 hours
        );

        uint256 sellerBefore = SELLER.balance;
        uint256 feeBefore = FEE_RECIPIENT.balance;

        vm.prank(BUYER);
        settlement.completeEscrow(ORDER_1);

        MarketSettlement.EscrowOrder memory order = settlement.getEscrow(ORDER_1);
        require(order.state == MarketSettlement.EscrowState.Completed, "State not completed");

        uint256 expectedFee = (1 ether * 250) / 10000;
        uint256 expectedSeller = 1 ether - expectedFee;

        require(SELLER.balance == sellerBefore + expectedSeller, "Seller payout mismatch");
        require(FEE_RECIPIENT.balance == feeBefore + expectedFee, "Fee payout mismatch");
    }

    function test_refundNativeEscrowAfterTimeout() public {
        vm.prank(BUYER);
        settlement.createEscrow{value: 1 ether}(
            ORDER_1,
            LISTING_1,
            SELLER,
            address(0),
            1 ether,
            250,
            2 hours
        );

        // Buyer cannot refund before timeout
        vm.expectRevert(abi.encodeWithSignature("Error(string)", "Timeout not expired"));
        vm.prank(BUYER);
        settlement.refundEscrow(ORDER_1, "Cancelled");

        // Warp past timeout
        vm.warp(block.timestamp + 2 hours + 1);

        uint256 buyerBefore = BUYER.balance;
        vm.prank(BUYER);
        settlement.refundEscrow(ORDER_1, "Timeout");

        require(BUYER.balance == buyerBefore + 1 ether, "Buyer refund mismatch");
        MarketSettlement.EscrowOrder memory order = settlement.getEscrow(ORDER_1);
        require(order.state == MarketSettlement.EscrowState.Refunded, "State not refunded");
    }

    function test_sellerCanRefundBuyerImmediately() public {
        vm.prank(BUYER);
        settlement.createEscrow{value: 1 ether}(
            ORDER_1,
            LISTING_1,
            SELLER,
            address(0),
            1 ether,
            0,
            2 hours
        );

        uint256 buyerBefore = BUYER.balance;
        vm.prank(SELLER);
        settlement.refundEscrow(ORDER_1, "Seller out of stock");

        require(BUYER.balance == buyerBefore + 1 ether, "Buyer refund mismatch");
    }

    function test_createTokenEscrowAndComplete() public {
        vm.startPrank(BUYER);
        token.approve(address(settlement), 1000);
        settlement.createEscrow(
            ORDER_1,
            LISTING_1,
            SELLER,
            address(token),
            1000,
            100, // 1%
            1 hours
        );
        vm.stopPrank();

        require(token.balanceOf(address(settlement)) == 1000, "Contract balance mismatch");

        vm.prank(BUYER);
        settlement.completeEscrow(ORDER_1);

        require(token.balanceOf(SELLER) == 990, "Seller token payout mismatch");
        require(token.balanceOf(FEE_RECIPIENT) == 10, "Fee recipient token payout mismatch");
    }

    function test_disputeAndOwnerResolve() public {
        vm.prank(BUYER);
        settlement.createEscrow{value: 1 ether}(
            ORDER_1,
            LISTING_1,
            SELLER,
            address(0),
            1 ether,
            500, // 5%
            2 hours
        );

        vm.prank(BUYER);
        settlement.disputeEscrow(ORDER_1, "Data damaged");

        MarketSettlement.EscrowOrder memory order = settlement.getEscrow(ORDER_1);
        require(order.state == MarketSettlement.EscrowState.Disputed, "State not disputed");

        // Resolve: 50% split to buyer, 50% to seller (minus fee)
        uint256 buyerBefore = BUYER.balance;
        uint256 sellerBefore = SELLER.balance;

        vm.prank(OWNER);
        settlement.resolveDispute(ORDER_1, 5000, "50-50 resolution");

        require(BUYER.balance == buyerBefore + 0.5 ether, "Buyer split mismatch");
        uint256 sellerPortion = 0.5 ether;
        uint256 expectedFee = (sellerPortion * 500) / 10000;
        require(SELLER.balance == sellerBefore + (sellerPortion - expectedFee), "Seller split mismatch");
    }

    function test_anchorMerkleRoot() public {
        bytes32 root = keccak256("merkle-root-v1");
        vm.prank(OWNER);
        settlement.anchorMerkleRoot(root, 42);

        require(settlement.anchoredRoots(root) == block.timestamp, "Root timestamp mismatch");

        // Cannot re-anchor identical root
        vm.expectRevert(abi.encodeWithSignature("Error(string)", "Root already anchored"));
        vm.prank(OWNER);
        settlement.anchorMerkleRoot(root, 42);
    }

    function test_revertUnauthorizedRootAnchor() public {
        bytes32 root = keccak256("merkle-root-v2");
        vm.expectRevert(abi.encodeWithSignature("Error(string)", "Only owner"));
        vm.prank(STRANGER);
        settlement.anchorMerkleRoot(root, 10);
    }
}
